//! OPY-to-Workshop integration, kept behind the `opy-rs` library boundary.
//!
//! This module pins the released `workshop-rs` v0.1.16 contract, checks the OPY
//! manifest links against the canonical catalog, and lowers the supported OPY
//! program structure into canonical WIR before validation and deterministic
//! Workshop emission.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::hir::{self, Expr, RuleEntry, Span as HirSpan, Stmt, SwitchArm, default_var_index};
use crate::manifest::{FunctionKind, Manifest};
use serde::Serialize;
use workshop_rs::catalog::{Catalog, CatalogIdentity, Kind, Locale};
use workshop_rs::source::{Position as WorkshopPosition, SourceFile, Span as WorkshopSpan};
use workshop_rs::wir::{self, Action, Event, PlayerEventKind, Program, Value, ValueNode};

pub mod reconstruct;

#[cfg(test)]
mod integration_tests;

/// The exact released dependency contract consumed by this crate.
pub const WORKSHOP_RS_VERSION: &str = "0.1.16";

const TRANSLATION_HELPER_NAME: &str = "__overpyTranslationHelper__";

/// Version of the machine-readable compile report contract.
pub const COMPILE_SCHEMA_VERSION: u32 = 1;

/// Stable identity of the compiler that produced a compile report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompilerIdentity {
    pub name: &'static str,
    pub version: &'static str,
}

/// Whether compilation produced a valid Workshop artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompileStatus {
    Success,
    Failure,
}

/// Stable classification for a compile failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompileFailureClass {
    Frontend,
    Integration,
}

/// A versioned, source-attributed diagnostic exposed by the compile API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileDiagnostic {
    pub severity: crate::tooling::DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub span: Option<crate::tooling::SourceLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<ScriptDiagnostic>,
}

/// The machine-readable result for one compile operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileResult {
    pub status: CompileStatus,
    pub exit_code: u8,
    pub failure_class: Option<CompileFailureClass>,
    pub diagnostics: Vec<CompileDiagnostic>,
    pub stdout: String,
    pub workshop_exact: String,
    pub workshop: String,
}

/// Complete versioned compile report for CLI, CI, and embedding consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileReport {
    pub schema_version: u32,
    pub compiler: CompilerIdentity,
    pub catalog: CatalogIdentity,
    pub compile: CompileResult,
}

impl CompilerIdentity {
    fn current() -> Self {
        Self {
            name: "opy-rs",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

/// A source-attributed integration diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationDiagnostic {
    pub code: String,
    pub message: String,
    pub span: Option<HirSpan>,
    pub script: Option<Box<ScriptDiagnostic>>,
}

/// Script-runtime provenance retained alongside the OPY directive anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDiagnostic {
    pub source_name: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub stack: Option<String>,
}

impl IntegrationDiagnostic {
    fn new(code: impl Into<String>, message: impl Into<String>, span: Option<HirSpan>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            span,
            script: None,
        }
    }
}

/// An integration boundary failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationError {
    pub diagnostic: IntegrationDiagnostic,
}

impl IntegrationError {
    fn new(code: impl Into<String>, message: impl Into<String>, span: Option<HirSpan>) -> Self {
        Self {
            diagnostic: IntegrationDiagnostic::new(code, message, span),
        }
    }

    fn post_compile_hook(error: crate::macro_js::MacroError, span: Option<HirSpan>) -> Self {
        let message = error.to_string();
        let script = match error {
            crate::macro_js::MacroError::Script(error) => Some(Box::new(ScriptDiagnostic {
                source_name: error.source_name,
                line: error.line,
                column: error.column,
                stack: error.stack,
            })),
            crate::macro_js::MacroError::InvalidResult { .. }
            | crate::macro_js::MacroError::Internal(_) => None,
        };
        Self {
            diagnostic: IntegrationDiagnostic {
                code: "post-compile-hook".to_string(),
                message,
                span,
                script,
            },
        }
    }
}

impl std::fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.diagnostic.code, self.diagnostic.message)
    }
}

impl std::error::Error for IntegrationError {}

/// Results of the manifest-to-catalog cross-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkReport {
    pub catalog_ids_checked: usize,
    pub domains_checked: usize,
}

/// Cross-check every OPY manifest `catalogId` and domain identity against the
/// canonical Workshop catalog. No local catalog copy or spelling allowlist is
/// involved.
pub(crate) fn cross_check_manifest(
    manifest: &Manifest,
    catalog: &Catalog,
) -> Result<LinkReport, IntegrationError> {
    let mut catalog_ids_checked = 0;
    let mut domains_checked = 0;

    for function in &manifest.functions {
        if let Some(catalog_id) = &function.catalog_id {
            let kind = match function.kind {
                FunctionKind::Action | FunctionKind::MemberAction => Kind::Action,
                FunctionKind::Value | FunctionKind::MemberValue => Kind::Value,
            };
            catalog_ids_checked += 1;
            if catalog.entry(kind, catalog_id).is_none() {
                return Err(IntegrationError::new(
                    "catalog-link-missing",
                    format!(
                        "manifest function '{}' links to missing {:?} catalog id '{}'",
                        function.id, kind, catalog_id
                    ),
                    None,
                ));
            }
        }

        for parameter in &function.params {
            let Some(domain) = &parameter.domain else {
                continue;
            };
            let contextual = function
                .contextual_domain
                .as_ref()
                .is_some_and(|context| context.domain == *domain);
            if contextual {
                continue;
            }
            domains_checked += 1;
            if catalog.enum_domain(domain).is_none() {
                return Err(IntegrationError::new(
                    "domain-link-missing",
                    format!(
                        "manifest function '{}' parameter '{}' links to missing enum domain '{}',",
                        function.id, parameter.name, domain
                    ),
                    None,
                ));
            }
        }

        if let Some(contextual) = &function.contextual_domain {
            for option in contextual.options.values() {
                domains_checked += 1;
                if catalog.enum_domain(&option.domain).is_none() {
                    return Err(IntegrationError::new(
                        "domain-link-missing",
                        format!(
                            "manifest function '{}' contextual option links to missing enum domain '{}'",
                            function.id, option.domain
                        ),
                        None,
                    ));
                }
            }
        }
    }

    Ok(LinkReport {
        catalog_ids_checked,
        domains_checked,
    })
}

/// The compiler-facing integration object. Construction validates the public
/// manifest/catalog contract once and exposes the pinned catalog identity.
pub struct Compiler {
    catalog: Catalog,
    manifest: &'static Manifest,
    links: LinkReport,
}

impl Compiler {
    pub fn new() -> Result<Self, IntegrationError> {
        let catalog = Catalog::builtin()
            .map_err(|error| IntegrationError::new("catalog-load", error.to_string(), None))?;
        let manifest = Manifest::builtin()
            .map_err(|error| IntegrationError::new("manifest-load", error.to_string(), None))?;
        let links = cross_check_manifest(manifest, &catalog)?;
        let identity = catalog.identity();
        if identity.implementation_version != WORKSHOP_RS_VERSION {
            return Err(IntegrationError::new(
                "workshop-contract-version",
                format!(
                    "expected workshop-rs {}, loaded {}",
                    WORKSHOP_RS_VERSION, identity.implementation_version
                ),
                None,
            ));
        }
        Ok(Self {
            catalog,
            manifest,
            links,
        })
    }

    pub fn catalog_identity(&self) -> CatalogIdentity {
        self.catalog.identity()
    }

    pub fn link_report(&self) -> LinkReport {
        self.links
    }

    /// Lower a resolved OPY HIR program into canonical WIR, validate it
    /// against the canonical catalog, and emit deterministic en-US Workshop.
    pub fn compile_hir(&self, hir: &hir::Program) -> Result<CompilationArtifact, IntegrationError> {
        self.compile_hir_with_locale(hir, &Locale::new("en-US"))
    }

    /// Lower and emit using a locale declared by the canonical catalog.
    pub fn compile_hir_with_locale(
        &self,
        hir: &hir::Program,
        locale: &Locale,
    ) -> Result<CompilationArtifact, IntegrationError> {
        if !self.catalog.supports(locale) {
            return Err(IntegrationError::new(
                "locale-unsupported",
                format!("workshop catalog does not declare locale '{locale}'"),
                None,
            ));
        }
        reject_unlowered_directives(hir)?;
        let expanded_hir = expand_macros(hir)?;
        let mut lowering = Lowering::new(self, &expanded_hir)?;
        lowering.copy_files()?;
        lowering.lower_declarations()?;
        lowering.lower_rules()?;

        lowering.wir.validate().map_err(|error| {
            let span = error
                .span()
                .and_then(|span| lowering.hir_span_from_workshop(span));
            IntegrationError::new(error.code(), error.message(), span)
        })?;
        workshop_rs::validate::validate_canonical_ids(&lowering.wir, &self.catalog).map_err(
            |error| {
                let span = workshop_error_span(&error)
                    .and_then(|span| lowering.hir_span_from_workshop(span));
                IntegrationError::new("catalog-validation", error.to_string(), span)
            },
        )?;
        let emitted =
            workshop_rs::emitter::emit(&lowering.wir, &self.catalog, locale).map_err(|error| {
                let span = workshop_error_span(&error)
                    .and_then(|span| lowering.hir_span_from_workshop(span));
                IntegrationError::new("workshop-emission", error.to_string(), span)
            })?;

        Ok(CompilationArtifact {
            wir: lowering.wir,
            final_output: emitted.clone(),
            emitted,
            catalog_identity: self.catalog.identity(),
            hook_console_output: Vec::new(),
        })
    }

    /// Compile source using the default `en-US` catalog locale.
    ///
    /// This is the ordinary embedding API. It returns Workshop text and does
    /// not require callers to construct a `workshop-rs` locale or understand
    /// canonical WIR types.
    pub fn compile_source(
        &self,
        source: &str,
        main_path: &str,
        root: &std::path::Path,
    ) -> Result<CompileOutput, IntegrationError> {
        self.compile_source_with_language(source, main_path, root, "en-US")
    }

    /// Compile source using a catalog locale name without exposing the
    /// `workshop-rs` locale type to ordinary embedding callers.
    pub fn compile_source_with_language(
        &self,
        source: &str,
        main_path: &str,
        root: &std::path::Path,
        language: &str,
    ) -> Result<CompileOutput, IntegrationError> {
        self.compile_source_with_locale(source, main_path, root, &Locale::new(language))
            .map(CompilationArtifact::into_output)
    }

    /// Compile source with an explicit canonical Workshop locale.
    ///
    /// This is an advanced integration API. Use [`Self::compile_source`] or
    /// [`Self::compile_source_with_language`] for ordinary embedding.
    pub fn compile_source_with_locale(
        &self,
        source: &str,
        main_path: &str,
        root: &std::path::Path,
        locale: &Locale,
    ) -> Result<CompilationArtifact, IntegrationError> {
        self.compile_source_internal(source, main_path, root, locale)
    }

    /// Compile source and return the canonical WIR artifact for advanced
    /// integrations.
    pub fn compile_source_artifact(
        &self,
        source: &str,
        main_path: &str,
        root: &std::path::Path,
    ) -> Result<CompilationArtifact, IntegrationError> {
        self.compile_source_with_locale(source, main_path, root, &Locale::new("en-US"))
    }

    /// Compile source into the versioned machine-readable result contract
    /// using the default `en-US` catalog locale.
    pub fn compile_source_report(
        &self,
        source: &str,
        main_path: &str,
        root: &std::path::Path,
    ) -> CompileReport {
        self.compile_source_report_with_language(source, main_path, root, "en-US")
    }

    /// Compile source into the versioned machine-readable result contract
    /// using a catalog locale name.
    pub fn compile_source_report_with_language(
        &self,
        source: &str,
        main_path: &str,
        root: &std::path::Path,
        language: &str,
    ) -> CompileReport {
        self.compile_source_report_with_locale(source, main_path, root, &Locale::new(language))
    }

    /// Compile source into the report contract with an explicit canonical
    /// Workshop locale. This is an advanced integration API.
    pub fn compile_source_report_with_locale(
        &self,
        source: &str,
        main_path: &str,
        root: &std::path::Path,
        locale: &Locale,
    ) -> CompileReport {
        let outcome = crate::compile_with_overlay_outcome(
            source,
            main_path,
            root,
            &std::collections::BTreeMap::new(),
        );
        let catalog = self.catalog.identity();
        let compiler = CompilerIdentity::current();
        let frontend_diagnostics = outcome
            .diagnostics
            .iter()
            .map(compile_frontend_diagnostic)
            .collect::<Vec<_>>();
        let Some(hir) = outcome.hir else {
            return CompileReport::failure(
                compiler,
                catalog,
                CompileFailureClass::Frontend,
                frontend_diagnostics,
            );
        };

        match self.compile_hir_with_locale_and_hook(&hir, outcome.post_compile_hook, locale) {
            Ok(artifact) => {
                CompileReport::success(compiler, catalog, artifact, frontend_diagnostics)
            }
            Err(error) => {
                let mut diagnostics = frontend_diagnostics;
                diagnostics.push(compile_diagnostic(error, &hir.files));
                CompileReport::failure(
                    compiler,
                    catalog,
                    CompileFailureClass::Integration,
                    diagnostics,
                )
            }
        }
    }

    fn compile_source_internal(
        &self,
        source: &str,
        main_path: &str,
        root: &std::path::Path,
        locale: &Locale,
    ) -> Result<CompilationArtifact, IntegrationError> {
        let outcome = crate::compile_with_overlay_outcome(
            source,
            main_path,
            root,
            &std::collections::BTreeMap::new(),
        );
        let hir = outcome.hir.ok_or_else(|| {
            let error = outcome
                .error
                .expect("failed frontend compile has diagnostic");
            IntegrationError::new(
                error.code,
                error.message,
                error.span.map(hir_span_from_diag),
            )
        })?;
        self.compile_hir_with_locale_and_hook(&hir, outcome.post_compile_hook, locale)
    }

    fn compile_hir_with_locale_and_hook(
        &self,
        hir: &hir::Program,
        hook: Option<crate::PostCompileHookRecord>,
        locale: &Locale,
    ) -> Result<CompilationArtifact, IntegrationError> {
        let mut artifact = self.compile_hir_with_locale(hir, locale)?;
        if let Some(hook) = hook {
            let runtime = crate::macro_js::MacroRuntime::new(crate::macro_js::Limits::default());
            let result = runtime
                .run_hook(&hook.source, &artifact.emitted, &hook.script)
                .map_err(|error| {
                    IntegrationError::post_compile_hook(error, hook.span.map(hir_span_from_diag))
                })?;
            artifact.final_output = result.text;
            artifact.hook_console_output = result.console_output;
        }
        Ok(artifact)
    }
}

impl CompileReport {
    fn success(
        compiler: CompilerIdentity,
        catalog: CatalogIdentity,
        artifact: CompilationArtifact,
        diagnostics: Vec<CompileDiagnostic>,
    ) -> Self {
        Self {
            schema_version: COMPILE_SCHEMA_VERSION,
            compiler,
            catalog,
            compile: CompileResult {
                status: CompileStatus::Success,
                exit_code: 0,
                failure_class: None,
                diagnostics,
                stdout: String::new(),
                workshop_exact: artifact.final_output.clone(),
                workshop: normalize_workshop(&artifact.final_output),
            },
        }
    }

    fn failure(
        compiler: CompilerIdentity,
        catalog: CatalogIdentity,
        failure_class: CompileFailureClass,
        diagnostics: Vec<CompileDiagnostic>,
    ) -> Self {
        Self {
            schema_version: COMPILE_SCHEMA_VERSION,
            compiler,
            catalog,
            compile: CompileResult {
                status: CompileStatus::Failure,
                exit_code: 1,
                failure_class: Some(failure_class),
                diagnostics,
                stdout: String::new(),
                workshop_exact: String::new(),
                workshop: String::new(),
            },
        }
    }
}

fn compile_diagnostic(error: IntegrationError, files: &[hir::SourceFile]) -> CompileDiagnostic {
    let diagnostic = error.diagnostic;
    CompileDiagnostic {
        severity: crate::tooling::DiagnosticSeverity::Error,
        code: diagnostic.code,
        message: diagnostic.message,
        span: diagnostic
            .span
            .and_then(|span| source_location_from_hir(span, files)),
        script: diagnostic.script.map(|script| *script),
    }
}

fn compile_frontend_diagnostic(diagnostic: &crate::tooling::Diagnostic) -> CompileDiagnostic {
    CompileDiagnostic {
        severity: diagnostic.severity,
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        span: diagnostic.span.clone(),
        script: None,
    }
}

fn source_location_from_hir(
    span: HirSpan,
    files: &[hir::SourceFile],
) -> Option<crate::tooling::SourceLocation> {
    let path = files.iter().find(|file| file.id == span.file)?.path.clone();
    Some(crate::tooling::SourceLocation {
        file_id: span.file,
        path,
        start: crate::diag::Position::new(span.start.line, span.start.col),
        end: crate::diag::Position::new(span.end.line, span.end.col),
    })
}

fn normalize_workshop(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized
        .split('\n')
        .map(|line| line.trim_end_matches([' ', '\t']).to_owned())
        .collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

fn reject_unlowered_directives(hir: &hir::Program) -> Result<(), IntegrationError> {
    if let Some(replacement) = hir.preprocessing.replacements.first() {
        let span = hir
            .preprocessing
            .directives
            .iter()
            .find(|directive| directive.name.starts_with("replace"))
            .and_then(|directive| directive.span)
            .or(replacement.span);
        return Err(IntegrationError::new(
            "backend-directive-unsupported",
            format!(
                "replacement directive '{}' has no canonical workshop-rs lowering",
                replacement.value
            ),
            span,
        ));
    }
    if let Some(replacement) = hir
        .preprocessing
        .directives
        .iter()
        .find(|directive| directive.name.starts_with("replace"))
    {
        return Err(IntegrationError::new(
            "backend-directive-unsupported",
            format!(
                "replacement directive '{}' has no canonical workshop-rs lowering",
                replacement.name
            ),
            replacement.span,
        ));
    }
    Ok(())
}

type MacroBindings = HashMap<String, Expr>;

struct MacroExpander {
    macros: HashMap<String, (Vec<String>, Vec<Stmt>)>,
    stack: Vec<String>,
}

fn expand_macros(program: &hir::Program) -> Result<hir::Program, IntegrationError> {
    let macros = program
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            hir::Declaration::Macro {
                name, args, body, ..
            } => Some((name.clone(), (args.clone(), body.clone()))),
            _ => None,
        })
        .collect();
    let mut expander = MacroExpander {
        macros,
        stack: Vec::new(),
    };
    let mut expanded = program.clone();
    let bindings = MacroBindings::new();

    for declaration in &mut expanded.declarations {
        match declaration {
            hir::Declaration::GlobalVariable { initializer, .. }
            | hir::Declaration::PlayerVariable { initializer, .. } => {
                if let Some(initializer) = initializer {
                    **initializer = expander.expand_expr(initializer, &bindings)?;
                }
            }
            _ => {}
        }
    }
    for entry in &mut expanded.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for argument in &mut rule.event.args {
                    *argument = expander.expand_expr(argument, &bindings)?;
                }
                for condition in &mut rule.conditions {
                    *condition = expander.expand_expr(condition, &bindings)?;
                }
                rule.actions = expander.expand_stmts(&rule.actions, &bindings)?;
            }
            RuleEntry::SubroutineDef { body, .. } => {
                *body = expander.expand_stmts(body, &bindings)?;
            }
        }
    }
    Ok(expanded)
}

impl MacroExpander {
    fn expand_stmts(
        &mut self,
        statements: &[Stmt],
        bindings: &MacroBindings,
    ) -> Result<Vec<Stmt>, IntegrationError> {
        let mut expanded = Vec::new();
        for statement in statements {
            if let Stmt::Expr { expr, .. } = statement {
                if let Expr::MacroCall { name, args, span } = expr.as_ref() {
                    let args = args
                        .iter()
                        .map(|arg| self.expand_expr(arg, bindings))
                        .collect::<Result<Vec<_>, _>>()?;
                    expanded.extend(self.expand_macro_body(name, &args, *span)?);
                    continue;
                }
            }
            expanded.push(self.expand_stmt(statement, bindings)?);
        }
        Ok(expanded)
    }

    fn expand_stmt(
        &mut self,
        statement: &Stmt,
        bindings: &MacroBindings,
    ) -> Result<Stmt, IntegrationError> {
        Ok(match statement {
            Stmt::Expr { expr, span } => Stmt::Expr {
                expr: Box::new(self.expand_expr(expr, bindings)?),
                span: *span,
            },
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: Box::new(self.expand_expr(target, bindings)?),
                value: Box::new(self.expand_expr(value, bindings)?),
                span: *span,
            },
            Stmt::If {
                branches,
                r#else,
                span,
            } => Stmt::If {
                branches: branches
                    .iter()
                    .map(|branch| {
                        Ok(hir::types::IfBranch {
                            condition: Box::new(self.expand_expr(&branch.condition, bindings)?),
                            body: self.expand_stmts(&branch.body, bindings)?,
                        })
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?,
                r#else: r#else
                    .as_ref()
                    .map(|body| self.expand_stmts(body, bindings))
                    .transpose()?,
                span: *span,
            },
            Stmt::For {
                variable,
                iterable,
                body,
                span,
            } => Stmt::For {
                variable: Box::new(self.expand_expr(variable, bindings)?),
                iterable: Box::new(self.expand_expr(iterable, bindings)?),
                body: self.expand_stmts(body, bindings)?,
                span: *span,
            },
            Stmt::While {
                condition,
                body,
                span,
            } => Stmt::While {
                condition: Box::new(self.expand_expr(condition, bindings)?),
                body: self.expand_stmts(body, bindings)?,
                span: *span,
            },
            Stmt::DoWhile {
                condition,
                body,
                span,
            } => Stmt::DoWhile {
                condition: Box::new(self.expand_expr(condition, bindings)?),
                body: self.expand_stmts(body, bindings)?,
                span: *span,
            },
            Stmt::Switch { value, arms, span } => Stmt::Switch {
                value: Box::new(self.expand_expr(value, bindings)?),
                arms: arms
                    .iter()
                    .map(|arm| match arm {
                        SwitchArm::Case { value, body, span } => Ok(SwitchArm::Case {
                            value: Box::new(self.expand_expr(value, bindings)?),
                            body: self.expand_stmts(body, bindings)?,
                            span: *span,
                        }),
                        SwitchArm::Default { body, span } => Ok(SwitchArm::Default {
                            body: self.expand_stmts(body, bindings)?,
                            span: *span,
                        }),
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?,
                span: *span,
            },
            Stmt::Delete { target, span } => Stmt::Delete {
                target: Box::new(self.expand_expr(target, bindings)?),
                span: *span,
            },
            Stmt::Goto {
                label,
                offset,
                rule_start,
                span,
            } => Stmt::Goto {
                label: label.clone(),
                offset: offset
                    .as_ref()
                    .map(|offset| self.expand_expr(offset, bindings).map(Box::new))
                    .transpose()?,
                rule_start: *rule_start,
                span: *span,
            },
            Stmt::Break { .. } | Stmt::CallSubroutine { .. } | Stmt::Pass { .. } => {
                statement.clone()
            }
            Stmt::Continue { .. } | Stmt::Label { .. } => statement.clone(),
        })
    }

    fn expand_expr(
        &mut self,
        expression: &Expr,
        bindings: &MacroBindings,
    ) -> Result<Expr, IntegrationError> {
        match expression {
            Expr::MacroParam { name, span } => bindings.get(name).cloned().ok_or_else(|| {
                IntegrationError::new(
                    "unsupported-integration-surface",
                    format!("macro parameter '{name}' has no expansion binding"),
                    *span,
                )
            }),
            Expr::MacroCall { name, args, span } => {
                let args = args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?;
                let body = self.expand_macro_body(name, &args, *span)?;
                if body.len() != 1 {
                    return Err(IntegrationError::new(
                        "macro-invalid",
                        format!("macro '{name}' must produce one expression in value position"),
                        *span,
                    ));
                }
                match body.into_iter().next().expect("one macro body statement") {
                    Stmt::Expr { expr, .. } => Ok(*expr),
                    _ => Err(IntegrationError::new(
                        "macro-invalid",
                        format!("macro '{name}' must produce an expression in value position"),
                        *span,
                    )),
                }
            }
            Expr::Array { elements, span } => Ok(Expr::Array {
                elements: elements
                    .iter()
                    .map(|element| self.expand_expr(element, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::Dict { entries, span } => Ok(Expr::Dict {
                entries: entries
                    .iter()
                    .map(|entry| {
                        Ok(hir::DictEntry {
                            key: Box::new(self.expand_expr(&entry.key, bindings)?),
                            value: Box::new(self.expand_expr(&entry.value, bindings)?),
                            span: entry.span,
                        })
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?,
                span: *span,
            }),
            Expr::Comprehension {
                element,
                variable,
                variable_span,
                index,
                index_span,
                iterable,
                condition,
                span,
            } => Ok(Expr::Comprehension {
                element: Box::new(self.expand_expr(element, bindings)?),
                variable: variable.clone(),
                variable_span: *variable_span,
                index: index.clone(),
                index_span: *index_span,
                iterable: Box::new(self.expand_expr(iterable, bindings)?),
                condition: condition
                    .as_ref()
                    .map(|condition| self.expand_expr(condition, bindings).map(Box::new))
                    .transpose()?,
                span: *span,
            }),
            Expr::Lambda {
                params,
                param_spans,
                body,
                span,
            } => Ok(Expr::Lambda {
                params: params.clone(),
                param_spans: param_spans.clone(),
                body: Box::new(self.expand_expr(body, bindings)?),
                span: *span,
            }),
            Expr::Type { name, args, span } => Ok(Expr::Type {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::Vector { x, y, z, span } => Ok(Expr::Vector {
                x: Box::new(self.expand_expr(x, bindings)?),
                y: Box::new(self.expand_expr(y, bindings)?),
                z: Box::new(self.expand_expr(z, bindings)?),
                span: *span,
            }),
            Expr::PlayerVar {
                player,
                name,
                member_span,
                span,
            } => Ok(Expr::PlayerVar {
                player: Box::new(self.expand_expr(player, bindings)?),
                name: name.clone(),
                member_span: *member_span,
                span: *span,
            }),
            Expr::Member {
                receiver,
                member,
                member_span,
                span,
            } => Ok(Expr::Member {
                receiver: Box::new(self.expand_expr(receiver, bindings)?),
                member: member.clone(),
                member_span: *member_span,
                span: *span,
            }),
            Expr::Call { name, args, span } => Ok(Expr::Call {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                span,
            } => Ok(Expr::ReceiverCall {
                receiver: Box::new(self.expand_expr(receiver, bindings)?),
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => Ok(Expr::Binary {
                op: op.clone(),
                left: Box::new(self.expand_expr(left, bindings)?),
                right: Box::new(self.expand_expr(right, bindings)?),
                span: *span,
            }),
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                span,
            } => Ok(Expr::Conditional {
                then_value: Box::new(self.expand_expr(then_value, bindings)?),
                condition: Box::new(self.expand_expr(condition, bindings)?),
                else_value: Box::new(self.expand_expr(else_value, bindings)?),
                span: *span,
            }),
            Expr::Unary { op, operand, span } => Ok(Expr::Unary {
                op: op.clone(),
                operand: Box::new(self.expand_expr(operand, bindings)?),
                span: *span,
            }),
            Expr::Index { array, index, span } => Ok(Expr::Index {
                array: Box::new(self.expand_expr(array, bindings)?),
                index: Box::new(self.expand_expr(index, bindings)?),
                span: *span,
            }),
            Expr::Format { text, args, span } => Ok(Expr::Format {
                text: text.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            _ => Ok(expression.clone()),
        }
    }

    fn expand_macro_body(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<Vec<Stmt>, IntegrationError> {
        let Some((params, body)) = self.macros.get(name).cloned() else {
            return Err(IntegrationError::new(
                "unsupported-integration-surface",
                format!("macro '{name}' has no declaration"),
                span,
            ));
        };
        if params.len() != args.len() {
            return Err(IntegrationError::new(
                "macro-arity",
                format!(
                    "macro '{name}' expects {} argument(s) but got {}",
                    params.len(),
                    args.len()
                ),
                span,
            ));
        }
        if self.stack.iter().any(|active| active == name) {
            return Err(IntegrationError::new(
                "macro-recursion",
                format!("recursive macro expansion detected for '{name}'"),
                span,
            ));
        }
        let mut bindings = MacroBindings::new();
        for (param, arg) in params.into_iter().zip(args.iter()) {
            bindings.insert(param, arg.clone());
        }
        self.stack.push(name.to_string());
        let result = self.expand_stmts(&body, &bindings);
        self.stack.pop();
        result
    }
}

/// A source compile result for ordinary embedding callers.
///
/// The result contains only emitted text and hook output. Callers that need
/// canonical WIR should use the explicit advanced artifact APIs instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOutput {
    /// Workshop text after a declared post-compile hook, if any.
    pub workshop: String,
    /// Workshop text emitted before a declared post-compile hook.
    pub emitted_workshop: String,
    /// Console lines captured while running a declared post-compile hook.
    pub hook_console_output: Vec<String>,
}

/// A validated WIR program and its emitted Workshop artifact for advanced
/// integrations.
pub struct CompilationArtifact {
    pub wir: Program,
    pub emitted: String,
    pub catalog_identity: CatalogIdentity,
    pub final_output: String,
    pub hook_console_output: Vec<String>,
}

impl CompilationArtifact {
    fn into_output(self) -> CompileOutput {
        CompileOutput {
            workshop: self.final_output,
            emitted_workshop: self.emitted,
            hook_console_output: self.hook_console_output,
        }
    }
}

fn convert_settings(settings: crate::hir::Settings) -> workshop_rs::settings::Settings {
    workshop_rs::settings::Settings {
        span: settings.span.map(convert_settings_span),
        children: settings
            .children
            .into_iter()
            .map(convert_settings_node)
            .collect(),
    }
}

fn convert_settings_node(node: crate::hir::SettingsNode) -> workshop_rs::settings::SettingsNode {
    use crate::hir::SettingsNode as SourceNode;
    use workshop_rs::settings::{SettingsListElement, SettingsNode as TargetNode};

    match node {
        SourceNode::Group {
            name,
            children,
            span,
        } => TargetNode::Group {
            name,
            children: children.into_iter().map(convert_settings_node).collect(),
            span: span.map(convert_settings_span),
        },
        SourceNode::Number { name, value, span } => TargetNode::Number {
            name,
            value,
            span: span.map(convert_settings_span),
        },
        SourceNode::Bool { name, value, span } => TargetNode::Bool {
            name,
            value,
            span: span.map(convert_settings_span),
        },
        SourceNode::String { name, value, span } => TargetNode::String {
            name,
            value,
            span: span.map(convert_settings_span),
        },
        SourceNode::List {
            name,
            elements,
            span,
        } => TargetNode::List {
            name,
            elements: elements
                .into_iter()
                .map(|element| SettingsListElement {
                    value: element.value,
                    span: element.span.map(convert_settings_span),
                })
                .collect(),
            span: span.map(convert_settings_span),
        },
    }
}

fn convert_settings_span(span: HirSpan) -> WorkshopSpan {
    WorkshopSpan::new(
        workshop_rs::source::FileId::from_index(span.file as usize),
        WorkshopPosition::new(span.start.line, span.start.col),
        WorkshopPosition::new(span.end.line, span.end.col),
    )
}

struct Lowering<'a> {
    compiler: &'a Compiler,
    hir: &'a hir::Program,
    wir: Program,
    files: HashMap<u32, workshop_rs::source::FileId>,
    wir_to_hir_files: Vec<u32>,
    globals: HashMap<String, wir::GlobalVarId>,
    players: HashMap<String, wir::PlayerVarId>,
    subroutines: HashMap<String, wir::SubroutineId>,
    constants: HashMap<String, &'a Expr>,
    defined_subroutines: HashSet<wir::SubroutineId>,
    array_bindings: Vec<ArrayBinding>,
    current_rule_conditions: Option<Vec<wir::ValueId>>,
}

#[derive(Debug, Clone)]
struct ArrayBinding {
    element: String,
    index: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum BreakTarget {
    Loop,
    DoWhile,
    Switch,
}

type SwitchBreak = (usize, HirSpan);
type LoweredSwitchBody = (Vec<wir::ActionId>, Option<SwitchBreak>);

impl<'a> Lowering<'a> {
    fn new(compiler: &'a Compiler, hir: &'a hir::Program) -> Result<Self, IntegrationError> {
        Ok(Self {
            compiler,
            hir,
            wir: Program::default(),
            files: HashMap::new(),
            wir_to_hir_files: Vec::new(),
            globals: HashMap::new(),
            players: HashMap::new(),
            subroutines: HashMap::new(),
            constants: HashMap::new(),
            defined_subroutines: HashSet::new(),
            array_bindings: Vec::new(),
            current_rule_conditions: None,
        })
    }

    fn copy_files(&mut self) -> Result<(), IntegrationError> {
        self.wir.settings = self.hir.settings.clone().map(convert_settings);
        for file in &self.hir.files {
            if self.files.contains_key(&file.id) {
                return Err(IntegrationError::new(
                    "source-file",
                    format!("duplicate HIR source file id {}", file.id),
                    None,
                ));
            }
            let id = self.wir.files.push(SourceFile::new(file.path.clone()));
            self.files.insert(file.id, id);
            self.wir_to_hir_files.push(file.id);
        }
        Ok(())
    }

    fn translation_helper_index(
        &self,
        implicit_reserved: &HashSet<u32>,
    ) -> Result<Option<u32>, IntegrationError> {
        if self.hir.preprocessing.translations.is_none() {
            return Ok(None);
        }
        let mut reserved = implicit_reserved.clone();
        reserved.extend(
            self.hir
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    hir::Declaration::GlobalVariable {
                        index: Some(index), ..
                    } => Some(*index),
                    _ => None,
                }),
        );
        (0..=127)
            .rev()
            .find(|index| !reserved.contains(index))
            .map(Some)
            .ok_or_else(|| {
                IntegrationError::new(
                    "index-exhausted",
                    "no available global variable index remains for translations",
                    self.hir
                        .preprocessing
                        .translations
                        .as_ref()
                        .and_then(|value| value.span),
                )
            })
    }

    fn lower_declarations(&mut self) -> Result<(), IntegrationError> {
        let (implicit_globals, implicit_players) = implicit_default_variables(self.hir);
        for declaration in &self.hir.declarations {
            if let hir::Declaration::GlobalVariable {
                name,
                index: Some(index),
                span,
                ..
            } = declaration
            {
                for (implicit_name, implicit_span) in &implicit_globals {
                    if default_var_index(implicit_name) == Some(*index) {
                        return Err(IntegrationError::new(
                            "index-collision",
                            format!(
                                "duplicate use of index {index} for global variables '{implicit_name}' and '{name}'"
                            ),
                            implicit_span.or(*span),
                        ));
                    }
                }
            }
            if let hir::Declaration::PlayerVariable {
                name,
                index: Some(index),
                span,
                ..
            } = declaration
            {
                for (implicit_name, implicit_span) in &implicit_players {
                    if default_var_index(implicit_name) == Some(*index) {
                        return Err(IntegrationError::new(
                            "index-collision",
                            format!(
                                "duplicate use of index {index} for player variables '{implicit_name}' and '{name}'"
                            ),
                            implicit_span.or(*span),
                        ));
                    }
                }
            }
        }

        let globals = self
            .hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::GlobalVariable { index, span, .. } => Some((*index, *span)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let players = self
            .hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::PlayerVariable { index, span, .. } => Some((*index, *span)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let subroutines = self
            .hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::Subroutine { index, span, .. } => Some((*index, *span)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let implicit_reserved = implicit_globals
            .keys()
            .map(|name| default_var_index(name).expect("implicit default variable names resolve"))
            .collect::<HashSet<_>>();
        let implicit_player_reserved = implicit_players
            .keys()
            .map(|name| default_var_index(name).expect("implicit default player names resolve"))
            .collect::<HashSet<_>>();
        let translation_helper_index = self.translation_helper_index(&implicit_reserved)?;
        let mut global_reserved = implicit_reserved.clone();
        if let Some(index) = translation_helper_index {
            global_reserved.insert(index);
        }
        let empty = HashSet::new();
        let global_indices = allocate_indices(&globals, &global_reserved, "global variable")?;
        let player_indices =
            allocate_indices(&players, &implicit_player_reserved, "player variable")?;
        let subroutine_indices = allocate_indices(&subroutines, &empty, "subroutine")?;
        let mut global_index = 0;
        let mut player_index = 0;
        let mut subroutine_index = 0;

        // Declared variables in source order (for duplicate detection and
        // initializer action order), then merged with the implicit default
        // variables and created in Workshop index order so the emitted
        // variable tables are reference-compatible.
        let mut declared_globals: Vec<(&str, u32, Option<HirSpan>, Option<HirSpan>)> = Vec::new();
        let mut global_initializers = Vec::new();
        let mut declared_players: Vec<(&str, u32, Option<HirSpan>, Option<HirSpan>)> = Vec::new();
        let mut player_initializers = Vec::new();
        let mut declared_subroutines: Vec<(&str, u32, Option<HirSpan>, Option<HirSpan>)> =
            Vec::new();

        for declaration in &self.hir.declarations {
            match declaration {
                hir::Declaration::GlobalVariable {
                    name,
                    index: _,
                    span,
                    name_span,
                    initializer,
                } => {
                    let assigned = global_indices[global_index];
                    global_index += 1;
                    if declared_globals
                        .iter()
                        .any(|(existing, ..)| *existing == name)
                    {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate global variable '{name}'"),
                            *span,
                        ));
                    }
                    declared_globals.push((name, assigned, *span, *name_span));
                    if let Some(init) = initializer {
                        if !is_zero_initializer(init) {
                            global_initializers.push((name, init, *span, *name_span));
                        }
                    }
                }
                hir::Declaration::PlayerVariable {
                    name,
                    index: _,
                    span,
                    name_span,
                    initializer,
                } => {
                    let assigned = player_indices[player_index];
                    player_index += 1;
                    if declared_players
                        .iter()
                        .any(|(existing, ..)| *existing == name)
                    {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate player variable '{name}'"),
                            *span,
                        ));
                    }
                    declared_players.push((name, assigned, *span, *name_span));
                    if let Some(init) = initializer {
                        if !is_zero_initializer(init) {
                            player_initializers.push((name, init, *span, *name_span));
                        }
                    }
                }
                hir::Declaration::Subroutine {
                    name,
                    span,
                    name_span,
                    ..
                } => {
                    let assigned = subroutine_indices[subroutine_index];
                    subroutine_index += 1;
                    if declared_subroutines
                        .iter()
                        .any(|(existing, ..)| *existing == name)
                    {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate subroutine '{name}'"),
                            *span,
                        ));
                    }
                    declared_subroutines.push((name, assigned, *span, *name_span));
                }
                hir::Declaration::Constant { name, value, span } => {
                    if self.constants.insert(name.clone(), value).is_some() {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate constant '{name}'"),
                            *span,
                        ));
                    }
                }
                hir::Declaration::Macro { .. } => {
                    // Macro definitions are retained for source tooling; calls
                    // are expanded before this WIR lowering pass.
                }
            }
        }

        let mut planned_globals: Vec<(String, u32, Option<HirSpan>, Option<HirSpan>)> =
            declared_globals
                .into_iter()
                .map(|(name, index, span, name_span)| (name.to_string(), index, span, name_span))
                .collect();
        planned_globals.extend(implicit_globals.iter().map(|(name, span)| {
            (
                name.clone(),
                default_var_index(name).expect("implicit default variable names resolve"),
                *span,
                None,
            )
        }));
        if let Some(index) = translation_helper_index {
            planned_globals.push((TRANSLATION_HELPER_NAME.to_string(), index, None, None));
        }
        planned_globals.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in planned_globals {
            let id = self.wir.global_variables.push(wir::WorkshopVariable {
                name: name.clone(),
                index: assigned,
                span: self.wir_span(span)?,
                name_span: self.wir_span(name_span)?,
            });
            self.globals.insert(name.clone(), id);
        }

        let mut planned_players: Vec<(String, u32, Option<HirSpan>, Option<HirSpan>)> =
            declared_players
                .into_iter()
                .map(|(name, index, span, name_span)| (name.to_string(), index, span, name_span))
                .collect();
        planned_players.extend(implicit_players.iter().map(|(name, span)| {
            (
                name.clone(),
                default_var_index(name).expect("implicit default player names resolve"),
                *span,
                None,
            )
        }));
        planned_players.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in planned_players {
            let id = self.wir.player_variables.push(wir::WorkshopVariable {
                name: name.clone(),
                index: assigned,
                span: self.wir_span(span)?,
                name_span: self.wir_span(name_span)?,
            });
            self.players.insert(name, id);
        }

        declared_subroutines.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in declared_subroutines {
            let id = self.wir.subroutines.push(wir::WorkshopSubroutine {
                name: name.to_string(),
                index: assigned,
                span: self.wir_span(span)?,
                name_span: self.wir_span(name_span)?,
            });
            self.subroutines.insert(name.to_string(), id);
        }

        let translation_initializer = self
            .hir
            .preprocessing
            .translations
            .as_ref()
            .map(|translations| {
                let variable = *self
                    .globals
                    .get(TRANSLATION_HELPER_NAME)
                    .expect("translation helper variable is created");
                let value = self.lower_translation_helper(translations)?;
                Ok(self.wir.actions.push(Action::SetGlobalVariable {
                    variable,
                    value,
                    span: self.wir_span(translations.span)?,
                    target_span: None,
                }))
            })
            .transpose()?;

        if translation_initializer.is_some() || !global_initializers.is_empty() {
            let mut actions = Vec::with_capacity(
                global_initializers.len() + usize::from(translation_initializer.is_some()),
            );
            if let Some(action) = translation_initializer {
                actions.push(action);
            }
            for (name, init_expr, span, target_span) in global_initializers {
                let variable = *self.globals.get(name).expect("declared global is created");
                let value = self.lower_value(init_expr)?;
                actions.push(self.wir.actions.push(Action::SetGlobalVariable {
                    variable,
                    value,
                    span: self.wir_span(span)?,
                    target_span: self.wir_span(target_span)?,
                }));
            }
            self.wir.rules.push(wir::Rule {
                name: self.global_initializer_rule_name(),
                span: None,
                name_span: None,
                disabled: false,
                event: Event::Global,
                conditions: Vec::new(),
                actions,
            });
        }

        if !player_initializers.is_empty() {
            let mut actions = Vec::with_capacity(player_initializers.len());
            for (name, init_expr, span, target_span) in player_initializers {
                let variable = *self
                    .players
                    .get(name)
                    .expect("declared player variable is created");
                let player = self
                    .wir
                    .values
                    .push(ValueNode::new(Value::EventPlayer, None));
                let value = self.lower_value(init_expr)?;
                actions.push(self.wir.actions.push(Action::SetPlayerVariable {
                    player,
                    variable,
                    value,
                    span: self.wir_span(span)?,
                    target_span: self.wir_span(target_span)?,
                }));
            }
            self.wir.rules.push(wir::Rule {
                name: "Initialize player variables".to_string(),
                span: None,
                name_span: None,
                disabled: false,
                event: Event::EachPlayer,
                conditions: Vec::new(),
                actions,
            });
        }

        Ok(())
    }

    fn lower_rules(&mut self) -> Result<(), IntegrationError> {
        for entry in &self.hir.rules {
            match entry {
                RuleEntry::Rule(rule) => self.lower_rule(rule)?,
                RuleEntry::SubroutineDef {
                    name,
                    source_name,
                    span,
                    name_span,
                    body,
                    annotations,
                    ..
                } => {
                    self.lower_subroutine(name, source_name, *span, *name_span, body, annotations)?
                }
            }
        }
        Ok(())
    }

    fn lower_rule(&mut self, rule: &hir::Rule) -> Result<(), IntegrationError> {
        self.reject_rule_metadata(rule)?;
        let event = self.lower_event(&rule.event, &rule.annotations)?;
        let conditions = rule
            .conditions
            .iter()
            .map(|expr| self.lower_condition(expr))
            .collect::<Result<Vec<_>, _>>()?;
        let previous_conditions = self.current_rule_conditions.replace(conditions.clone());
        let lowered_actions = self.lower_actions(&rule.actions, None);
        self.current_rule_conditions = previous_conditions;
        let mut actions = Vec::new();
        actions.extend(lowered_actions?);
        self.wir.rules.push(wir::Rule {
            name: rule.name.clone(),
            span: self.wir_span(rule.span)?,
            name_span: self.wir_span(rule.name_span)?,
            disabled: rule.disabled,
            event,
            conditions,
            actions,
        });
        Ok(())
    }

    fn lower_subroutine(
        &mut self,
        name: &str,
        source_name: &str,
        span: Option<HirSpan>,
        name_span: Option<HirSpan>,
        body: &[Stmt],
        annotations: &[hir::Annotation],
    ) -> Result<(), IntegrationError> {
        self.reject_subroutine_metadata(annotations)?;
        let source_name = if source_name.is_empty() {
            name
        } else {
            source_name
        };
        let subroutine = *self.subroutines.get(source_name).ok_or_else(|| {
            self.unsupported(
                format!("subroutine definition '{source_name}' has no declaration"),
                name_span.or(span),
            )
        })?;
        if !self.defined_subroutines.insert(subroutine) {
            return Err(self.unsupported(
                format!("subroutine '{source_name}' has multiple definitions"),
                name_span.or(span),
            ));
        }
        let mut actions = Vec::new();
        actions.extend(self.lower_actions(body, None)?);
        self.wir.rules.push(wir::Rule {
            name: self.subroutine_rule_name(name),
            span: self.wir_span(span)?,
            name_span: self.wir_span(name_span)?,
            disabled: false,
            event: Event::Subroutine(subroutine),
            conditions: Vec::new(),
            actions,
        });
        Ok(())
    }

    fn reject_rule_metadata(&self, rule: &hir::Rule) -> Result<(), IntegrationError> {
        if rule.delimiter {
            let span = rule
                .annotations
                .iter()
                .find(|annotation| annotation.name == "Delimiter")
                .and_then(|annotation| annotation.span)
                .or(rule.span);
            return Err(self.unsupported(
                "rule delimiter metadata is not representable in canonical WIR",
                span,
            ));
        }
        if rule.new_page.is_some() {
            let span = rule
                .annotations
                .iter()
                .find(|annotation| annotation.name == "NewPage")
                .and_then(|annotation| annotation.span)
                .or(rule.span);
            return Err(self.unsupported(
                "rule new-page metadata is not representable in canonical WIR",
                span,
            ));
        }
        for annotation in &rule.annotations {
            match annotation.name.as_str() {
                "Event" | "Condition" | "Team" | "Slot" | "Hero" | "Disabled"
                | "SuppressWarnings" => {}
                _ => {
                    return Err(self.unsupported(
                        format!(
                            "rule annotation '{}' is not representable in canonical WIR",
                            annotation.name
                        ),
                        annotation.span.or(rule.span),
                    ));
                }
            }
        }
        Ok(())
    }

    fn reject_subroutine_metadata(
        &self,
        annotations: &[hir::Annotation],
    ) -> Result<(), IntegrationError> {
        for annotation in annotations {
            match annotation.name.as_str() {
                "Name" | "SuppressWarnings" => {}
                _ => {
                    return Err(self.unsupported(
                        format!(
                            "subroutine annotation '{}' is not representable in canonical WIR",
                            annotation.name
                        ),
                        annotation.span,
                    ));
                }
            }
        }
        Ok(())
    }

    fn subroutine_rule_name(&self, generated_name: &str) -> String {
        if self.hir.preprocessing.rule_prefix_template.is_some() {
            generated_name.to_string()
        } else {
            format!("Subroutine {generated_name}")
        }
    }

    fn global_initializer_rule_name(&self) -> String {
        if self.hir.preprocessing.rule_prefix_template.is_some() {
            "[] Initialize global variables".to_string()
        } else {
            "Initialize global variables".to_string()
        }
    }

    fn lower_event(
        &self,
        event: &hir::Event,
        annotations: &[hir::Annotation],
    ) -> Result<Event, IntegrationError> {
        if !event.args.is_empty() {
            return Err(self.unsupported(
                "event arguments are not representable in canonical WIR; use structural event filters",
                event.span,
            ));
        }
        let team = self.lower_event_team(annotations)?;
        let target = self.lower_event_target(annotations)?;
        let has_filters =
            !matches!(team, wir::EventTeam::All) || !matches!(target, wir::EventTarget::All);
        match event.name.as_str() {
            "global" => {
                if has_filters {
                    return Err(
                        self.unsupported("global events cannot have player filters", event.span)
                    );
                }
                Ok(Event::Global)
            }
            "eachPlayer" => {
                if has_filters {
                    Ok(Event::EachPlayerWithFilters { team, target })
                } else {
                    Ok(Event::EachPlayer)
                }
            }
            name => player_event_kind(name).map_or_else(
                || {
                    Err(self.unsupported(
                        format!("event '{name}' is not supported by canonical WIR"),
                        event.span,
                    ))
                },
                |kind| Ok(Event::Player { kind, team, target }),
            ),
        }
    }

    fn lower_event_team(
        &self,
        annotations: &[hir::Annotation],
    ) -> Result<wir::EventTeam, IntegrationError> {
        let team_annotations = annotations
            .iter()
            .filter(|annotation| annotation.name == "Team")
            .collect::<Vec<_>>();
        if team_annotations.len() > 1 {
            return Err(self.unsupported(
                "an event cannot have multiple @Team filters",
                team_annotations[1].span.or(team_annotations[0].span),
            ));
        }
        let Some(annotation) = team_annotations.first() else {
            return Ok(wir::EventTeam::All);
        };
        let argument = annotation
            .args
            .first()
            .ok_or_else(|| self.unsupported("@Team requires one filter value", annotation.span))?;
        if annotation.args.len() != 1 {
            return Err(
                self.unsupported("@Team requires exactly one filter value", annotation.span)
            );
        }
        let spelling = match argument.text.as_str() {
            "1" => "Team 1",
            "2" => "Team 2",
            value => value,
        };
        let (_, member) = self
            .compiler
            .catalog
            .resolve_enum_member("EventTeam", &Locale::new("en-US"), spelling)
            .ok_or_else(|| {
                self.unsupported(
                    format!("unknown EventTeam filter '{spelling}'"),
                    argument.span.or(annotation.span),
                )
            })?;
        match member.as_str() {
            "ALL" => Ok(wir::EventTeam::All),
            "TEAM_1" => Ok(wir::EventTeam::Team1),
            "TEAM_2" => Ok(wir::EventTeam::Team2),
            _ => Err(self.unsupported(
                format!("catalog EventTeam member '{member}' is not supported by canonical WIR"),
                argument.span.or(annotation.span),
            )),
        }
    }

    fn lower_event_target(
        &self,
        annotations: &[hir::Annotation],
    ) -> Result<wir::EventTarget, IntegrationError> {
        let mut filters = Vec::new();
        for name in ["Slot", "Hero"] {
            let matches = annotations
                .iter()
                .filter(|annotation| annotation.name == name)
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return Err(self.unsupported(
                    format!("an event cannot have multiple @{name} filters"),
                    matches[1].span.or(matches[0].span),
                ));
            }
            filters.extend(matches);
        }
        if filters.len() > 1 {
            return Err(self.unsupported(
                "an event cannot combine @Slot and @Hero filters",
                filters[1].span.or(filters[0].span),
            ));
        }
        let Some(annotation) = filters.first() else {
            return Ok(wir::EventTarget::All);
        };
        let argument = annotation.args.first().ok_or_else(|| {
            self.unsupported(
                format!("@{} requires one filter value", annotation.name),
                annotation.span,
            )
        })?;
        if annotation.args.len() != 1 {
            return Err(self.unsupported(
                format!("@{} requires exactly one filter value", annotation.name),
                annotation.span,
            ));
        }
        let spelling = if annotation.name == "Slot" {
            match argument.text.as_str() {
                value if value.parse::<u8>().is_ok() => {
                    format!("Slot {}", value.parse::<u8>().unwrap_or_default())
                }
                value => value.to_string(),
            }
        } else {
            argument.text.clone()
        };
        let domain = if annotation.name == "Slot" {
            "EventPlayer"
        } else {
            "Hero"
        };
        let (_, member) = self
            .compiler
            .catalog
            .resolve_enum_member(domain, &Locale::new("en-US"), &spelling)
            .ok_or_else(|| {
                self.unsupported(
                    format!("unknown {domain} filter '{spelling}'"),
                    argument.span.or(annotation.span),
                )
            })?;
        if domain == "EventPlayer" {
            if member == "ALL" {
                Ok(wir::EventTarget::All)
            } else if let Some(slot) = member.strip_prefix("SLOT_") {
                let slot = slot.parse::<u8>().map_err(|_| {
                    self.unsupported(
                        format!("catalog EventPlayer member '{member}' is not a slot"),
                        argument.span.or(annotation.span),
                    )
                })?;
                Ok(wir::EventTarget::Slot(slot))
            } else {
                Err(self.unsupported(
                    format!(
                        "catalog EventPlayer member '{member}' is not supported by canonical WIR"
                    ),
                    argument.span.or(annotation.span),
                ))
            }
        } else {
            Ok(wir::EventTarget::Hero(member))
        }
    }

    fn lower_actions(
        &mut self,
        statements: &[Stmt],
        break_target: Option<BreakTarget>,
    ) -> Result<Vec<wir::ActionId>, IntegrationError> {
        let mut actions = Vec::new();
        for statement in statements {
            actions.extend(self.lower_action(statement, break_target)?);
        }
        Ok(actions)
    }

    fn lower_action(
        &mut self,
        stmt: &Stmt,
        break_target: Option<BreakTarget>,
    ) -> Result<Vec<wir::ActionId>, IntegrationError> {
        match stmt {
            Stmt::Pass { .. } => Ok(Vec::new()),
            Stmt::Assign {
                target,
                value,
                span,
            } => self.lower_assign(target, value, *span).map(|action| vec![action]),
            Stmt::If {
                branches,
                r#else,
                span,
            } => {
                let branches = branches
                    .iter()
                    .map(|branch| {
                        Ok(wir::IfBranch {
                            condition: self.lower_value(&branch.condition)?,
                            body: self.lower_actions(&branch.body, break_target)?,
                        })
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?;
                let else_body = r#else
                    .as_ref()
                    .map(|body| self.lower_actions(body, break_target))
                    .transpose()?;
                Ok(vec![self.wir.actions.push(Action::If {
                    branches,
                    else_body,
                    span: self.wir_span(*span)?,
                })])
            }
            Stmt::For {
                variable,
                iterable,
                body,
                span,
            } => {
                let (start, stop, step) = self.lower_range(iterable)?;
                let body = self.lower_actions(body, Some(BreakTarget::Loop))?;
                match variable.as_ref() {
                    Expr::GlobalVar {
                        name,
                        span: target_span,
                    } => {
                        let variable_id = *self.globals.get(name).ok_or_else(|| {
                            self.unsupported(
                                format!("unknown global variable '{name}'"),
                                *target_span,
                            )
                        })?;
                        Ok(vec![self.wir.actions.push(Action::ForGlobalVariable {
                            variable: variable_id,
                            start,
                            stop,
                            step,
                            body,
                            span: self.wir_span(*span)?,
                            target_span: self.wir_span(*target_span)?,
                        })])
                    }
                    Expr::PlayerVar {
                        player,
                        name,
                        span: target_span,
                        ..
                    } => {
                        let variable_id = *self.players.get(name).ok_or_else(|| {
                            self.unsupported(
                                format!("unknown player variable '{name}'"),
                                *target_span,
                            )
                        })?;
                        let player = self.lower_value(player)?;
                        Ok(vec![self.wir.actions.push(Action::ForPlayerVariable {
                            player,
                            variable: variable_id,
                            start,
                            stop,
                            step,
                            body,
                            span: self.wir_span(*span)?,
                        })])
                    }
                    _ => Err(self.unsupported(
                        "range loops require a global- or player-variable binder in canonical WIR",
                        variable.span().copied(),
                    )),
                }
            }
            Stmt::While {
                condition,
                body,
                span,
            } => {
                let condition = self.lower_value(condition)?;
                let body = self.lower_actions(body, Some(BreakTarget::Loop))?;
                Ok(vec![self.wir.actions.push(Action::While {
                    condition,
                    body,
                    span: self.wir_span(*span)?,
                })])
            }
            Stmt::DoWhile {
                condition,
                body,
                span,
            } => {
                let body = self.lower_do_while_body(body)?;
                let condition = self.lower_value(condition)?;
                let loop_if = self.wir.actions.push(Action::Call {
                    name: "loopIf".to_string(),
                    args: vec![condition],
                    span: self.wir_span(*span)?,
                });
                // OverPy's pinned lowering expands do/while into its body
                // followed by the canonical Loop If action.
                let mut actions = body;
                actions.push(loop_if);
                Ok(actions)
            }
            Stmt::Switch {
                value,
                arms,
                span,
            } => self.lower_switch(value, arms, *span).map(|action| vec![action]),
            Stmt::Delete { span, .. } => Err(self.unsupported(
                "delete statements are not representable in canonical WIR",
                *span,
            )),
            Stmt::Continue { span } => Err(self.unsupported(
                "continue statements are not representable in canonical WIR",
                *span,
            )),
            Stmt::Goto { span, .. } => Err(self.unsupported(
                "goto statements are not representable in canonical WIR",
                *span,
            )),
            Stmt::Label { span, .. } => Err(self.unsupported(
                "labels are not representable in canonical WIR",
                *span,
            )),
            Stmt::Break { span } => match break_target {
                Some(BreakTarget::Loop) => Ok(vec![self.wir.actions.push(Action::Call {
                    name: "break".to_string(),
                    args: Vec::new(),
                    span: self.wir_span(*span)?,
                })]),
                Some(BreakTarget::DoWhile) => Err(self.unsupported(
                    "break inside a do-while must be a direct statement or a single conditional break",
                    *span,
                )),
                Some(BreakTarget::Switch) => Err(self.unsupported(
                    "break inside a nested conditional cannot be normalized into canonical switch control flow",
                    *span,
                )),
                None => Err(self.unsupported(
                    "break has no enclosing canonical loop or switch",
                    *span,
                )),
            },
            Stmt::Expr { expr, span } => match expr.as_ref() {
                Expr::Call { name, args, .. } => {
                    if name == "disableInspector" && args.is_empty() {
                        Ok(vec![self.wir.actions.push(Action::Call {
                            name: "disableInspector".to_string(),
                            args: Vec::new(),
                            span: self.wir_span(*span)?,
                        })])
                    } else if name == "debug" && args.len() == 1 {
                        Ok(vec![self.lower_debug(&args[0], *span)?])
                    } else if name == "print" && args.len() == 1 {
                        Ok(vec![self.lower_print(&args[0], *span)?])
                    } else {
                        self.lower_action_call(name, args, *span).map(|action| vec![action])
                    }
                }
                Expr::ReceiverCall {
                    receiver,
                    name,
                    args,
                    span: call_span,
                } => self
                    .lower_receiver_action_call(receiver, name, args, *call_span)
                    .map(|action| vec![action]),
                _ => Err(self.unsupported(
                    "only action calls are currently representable as expression statements in canonical WIR",
                    *span,
                )),
            },
            Stmt::CallSubroutine { name, span } => {
                let subroutine = *self.subroutines.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown subroutine '{name}'"), *span)
                })?;
                let span = self.wir_span(*span)?;
                Ok(vec![self.wir.actions.push(Action::CallSubroutine {
                    subroutine,
                    span,
                    callee_span: span,
                })])
            }
        }
    }

    fn lower_do_while_body(
        &mut self,
        statements: &[Stmt],
    ) -> Result<Vec<wir::ActionId>, IntegrationError> {
        let mut actions = Vec::new();
        for (index, statement) in statements.iter().enumerate() {
            let direct_break = matches!(statement, Stmt::Break { .. });
            let conditional_break = match statement {
                Stmt::If {
                    branches,
                    r#else: None,
                    ..
                } if branches.len() == 1 => {
                    matches!(branches[0].body.as_slice(), [Stmt::Break { .. }])
                }
                _ => false,
            };

            if direct_break || conditional_break {
                let tail = self.lower_do_while_body(&statements[index + 1..])?;
                let distance = self.canonical_action_width(&tail, statement.span().copied())? + 1;
                let (name, args, span) = if let Stmt::Break { span } = statement {
                    ("skip", Vec::new(), *span)
                } else if let Stmt::If { branches, span, .. } = statement {
                    (
                        "skipIf",
                        vec![self.lower_value(&branches[0].condition)?],
                        *span,
                    )
                } else {
                    unreachable!("break shape was checked above")
                };
                let distance = self.wir.values.push(ValueNode::new(
                    Value::Number {
                        value: distance as f64,
                        text: distance.to_string(),
                    },
                    self.wir_span(span)?,
                ));
                let mut args = args;
                args.push(distance);
                actions.push(self.wir.actions.push(Action::Call {
                    name: name.to_string(),
                    args,
                    span: self.wir_span(span)?,
                }));
                actions.extend(tail);
                return Ok(actions);
            }

            actions.extend(self.lower_action(statement, Some(BreakTarget::DoWhile))?);
        }
        Ok(actions)
    }

    fn lower_range(
        &mut self,
        iterable: &Expr,
    ) -> Result<(wir::ValueId, wir::ValueId, wir::ValueId), IntegrationError> {
        let Expr::Call { name, args, .. } = iterable else {
            return Err(self.unsupported(
                "range loop iterable must be a range(...) call",
                iterable.span().copied(),
            ));
        };
        if name != "range" || !(1..=3).contains(&args.len()) {
            return Err(self.unsupported(
                "range loop requires one to three arguments",
                iterable.span().copied(),
            ));
        }
        let span = iterable.span().copied();
        let number = |this: &mut Self, value: f64| -> Result<wir::ValueId, IntegrationError> {
            Ok(this.wir.values.push(ValueNode::new(
                Value::Number {
                    value,
                    text: value.to_string(),
                },
                this.wir_span(span)?,
            )))
        };
        match args.as_slice() {
            [stop] => Ok((
                number(self, 0.0)?,
                self.lower_value(stop)?,
                number(self, 1.0)?,
            )),
            [start, stop] => Ok((
                self.lower_value(start)?,
                self.lower_value(stop)?,
                number(self, 1.0)?,
            )),
            [start, stop, step] => Ok((
                self.lower_value(start)?,
                self.lower_value(stop)?,
                self.lower_value(step)?,
            )),
            _ => unreachable!("range arity checked above"),
        }
    }

    fn lower_switch(
        &mut self,
        value: &Expr,
        arms: &[SwitchArm],
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
        let selector = self.lower_value(value)?;
        let mut case_values = Vec::new();
        let mut lowered_arms = Vec::with_capacity(arms.len());
        let mut case_offsets = Vec::new();
        let mut offset = 0usize;
        let mut default_offset = None;

        for arm in arms {
            let (value, (body, break_at)) = match arm {
                SwitchArm::Case { value, body, .. } => {
                    case_values.push(self.lower_value(value)?);
                    (Some(value), self.lower_switch_body(body)?)
                }
                SwitchArm::Default { body, span } => {
                    if default_offset.is_some() {
                        return Err(
                            self.unsupported("a switch may contain at most one default arm", *span)
                        );
                    }
                    default_offset = Some(offset);
                    (None, self.lower_switch_body(body)?)
                }
            };
            if value.is_some() {
                case_offsets.push(offset);
            }
            offset += self.canonical_action_width(&body, span)? + usize::from(break_at.is_some());
            lowered_arms.push((value, body, break_at));
        }
        let default_offset = default_offset.unwrap_or(offset);

        let break_arms: Vec<_> = lowered_arms
            .iter()
            .enumerate()
            .filter_map(|(index, (_, _, break_at))| break_at.map(|break_at| (index, break_at)))
            .collect();
        if break_arms.len() > 1 {
            let (first_index, first_break) = break_arms[0];
            let has_actions_after_first = lowered_arms[first_index].1.len() > first_break.0
                || lowered_arms
                    .iter()
                    .skip(first_index + 1)
                    .any(|(_, body, _)| !body.is_empty());
            if has_actions_after_first {
                return Err(self.unsupported(
                    "multiple switch breaks with later reachable actions require canonical switch targets",
                    Some(break_arms[1].1.1),
                ));
            }
        }

        let case_values = self.lower_array(case_values, span)?;
        let value_span = self.wir_span(span)?;
        let offset_values = std::iter::once(default_offset)
            .chain(case_offsets)
            .map(|value| {
                self.wir.values.push(ValueNode::new(
                    Value::Number {
                        value: value as f64,
                        text: value.to_string(),
                    },
                    value_span,
                ))
            })
            .collect();
        let offsets = self.lower_array(offset_values, span)?;
        let one = self.wir.values.push(ValueNode::new(
            Value::Number {
                value: 1.0,
                text: "1".to_string(),
            },
            self.wir_span(span)?,
        ));
        let index = self.wir.values.push(ValueNode::new(
            Value::Call {
                name: "indexOfArrayValue".to_string(),
                args: vec![case_values, selector],
            },
            self.wir_span(span)?,
        ));
        let case_offset = self.wir.values.push(ValueNode::new(
            Value::Call {
                name: "add".to_string(),
                args: vec![one, index],
            },
            self.wir_span(span)?,
        ));
        let skip_condition = self.wir.values.push(ValueNode::new(
            Value::Call {
                name: "valueInArray".to_string(),
                args: vec![offsets, case_offset],
            },
            self.wir_span(span)?,
        ));
        let skip = self.wir.actions.push(Action::Call {
            name: "skip".to_string(),
            args: vec![skip_condition],
            span: self.wir_span(span)?,
        });
        let true_value = self
            .wir
            .values
            .push(ValueNode::new(Value::Bool(true), self.wir_span(span)?));

        let first_break = break_arms.first().copied();
        let mut branch_body = vec![skip];
        let else_body = if let Some((break_index, (break_at, _))) = first_break {
            for (index, (_, body, _)) in lowered_arms.iter().enumerate() {
                if index < break_index {
                    branch_body.extend(body.iter().copied());
                } else if index == break_index {
                    branch_body.extend(body[..break_at].iter().copied());
                }
            }
            let mut tail = Vec::new();
            tail.extend(lowered_arms[break_index].1[break_at..].iter().copied());
            for (_, body, _) in lowered_arms.iter().skip(break_index + 1) {
                tail.extend(body.iter().copied());
            }
            Some(tail)
        } else {
            for (_, body, _) in &lowered_arms {
                branch_body.extend(body.iter().copied());
            }
            None
        };

        Ok(self.wir.actions.push(Action::If {
            branches: vec![wir::IfBranch {
                condition: true_value,
                body: branch_body,
            }],
            else_body,
            span: self.wir_span(span)?,
        }))
    }

    fn lower_switch_body(
        &mut self,
        statements: &[Stmt],
    ) -> Result<LoweredSwitchBody, IntegrationError> {
        let mut actions = Vec::new();
        let mut break_at = None;
        for statement in statements {
            if let Stmt::Break { span } = statement {
                if break_at.is_some() {
                    return Err(self.unsupported(
                        "multiple switch breaks in one arm require canonical switch targets",
                        *span,
                    ));
                }
                break_at = Some((
                    actions.len(),
                    span.ok_or_else(|| {
                        self.unsupported("switch break is missing source provenance", None)
                    })?,
                ));
                continue;
            }
            actions.extend(self.lower_action(statement, Some(BreakTarget::Switch))?);
        }
        Ok((actions, break_at))
    }

    /// Query the Workshop-owned native action layout for a relative jump.
    fn canonical_action_width(
        &self,
        actions: &[wir::ActionId],
        fallback_span: Option<HirSpan>,
    ) -> Result<usize, IntegrationError> {
        workshop_rs::emitter::action_width(
            &self.wir,
            &self.compiler.catalog,
            &Locale::new("en-US"),
            actions,
        )
        .map(|layout| layout.width)
        .map_err(|error| {
            let workshop_span = match &error {
                workshop_rs::emitter::ActionLayoutError::InvalidWIR(error) => error.span(),
                workshop_rs::emitter::ActionLayoutError::Emission(error) => {
                    workshop_error_span(error)
                }
            };
            let span = workshop_span
                .and_then(|span| self.hir_span_from_workshop(span))
                .or(fallback_span);
            IntegrationError::new("workshop-action-layout", error.to_string(), span)
        })
    }

    fn lower_array(
        &mut self,
        elements: Vec<wir::ValueId>,
        span: Option<HirSpan>,
    ) -> Result<wir::ValueId, IntegrationError> {
        let name = if elements.is_empty() {
            "emptyArray"
        } else {
            "array"
        };
        Ok(self.wir.values.push(ValueNode::new(
            Value::Call {
                name: name.to_string(),
                args: elements,
            },
            self.wir_span(span)?,
        )))
    }

    fn lower_translation_helper(
        &mut self,
        translations: &hir::TranslationState,
    ) -> Result<wir::ValueId, IntegrationError> {
        let translated_white = translations
            .languages
            .iter()
            .map(|language| {
                let locale = translation_locale(language).ok_or_else(|| {
                    IntegrationError::new(
                        "translations-invalid",
                        format!("unsupported translation language '{language}'"),
                        translations.span,
                    )
                })?;
                self.compiler
                    .catalog
                    .enum_spelling("Color", &Locale::new(locale), "WHITE")
                    .map(str::to_string)
                    .ok_or_else(|| {
                        IntegrationError::new(
                            "translations-invalid",
                            format!("catalog has no Color.WHITE spelling for locale '{locale}'"),
                            translations.span,
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("0");
        let text = self.push_value(Value::String(format!("\u{ec48}0{translated_white}")));
        let custom_string = self.push_call("customString", vec![text]);
        let null = self.push_value(Value::Null);
        let separator = self.push_call("firstOf", vec![null]);
        Ok(self.push_call("stringSplit", vec![custom_string, separator]))
    }

    fn lower_debug(
        &mut self,
        expr: &Expr,
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
        macro_rules! call {
            ($name:literal $(, $arg:expr)* $(,)?) => {{
                let args = vec![$($arg),*];
                self.push_call($name, args)
            }};
        }

        let value = self.lower_text_value(expr)?;
        let array_text = if self.debug_value_is_array(value) {
            self.lower_debug_array_text(value)
        } else {
            value
        };
        let debug_label = canonical_debug_text(&debug_expr_text(expr));
        let debug_prefix = format!("{debug_label}\u{2028}= {{0}}");
        let inline_padding = 128 - debug_prefix.chars().count() - "{1}".chars().count();
        let padding_text = self.push_value(Value::String(" ".repeat(170 - inline_padding)));
        let padding = self.push_call("customString", vec![padding_text]);
        let debug_label = self.push_value(Value::String(format!(
            "{debug_prefix}{}{{1}}",
            " ".repeat(inline_padding)
        )));
        let text = self.push_call("customString", vec![debug_label, array_text, padding]);
        let all_teams = self.push_value(Value::Enum {
            value_type: "Team".to_string(),
            value: "ALL".to_string(),
        });
        let all_players = call!("allPlayers", all_teams);
        let null_value = self.push_value(Value::Null);
        let null_value_2 = self.push_value(Value::Null);
        let null_value_3 = self.push_value(Value::Null);
        let null_value_4 = self.push_value(Value::Null);
        let hud_position = self.push_value(Value::Enum {
            value_type: "HudPosition".to_string(),
            value: "LEFT".to_string(),
        });
        let sort_order = self.push_value(Value::Number {
            value: -9999.0,
            text: "-9999".to_string(),
        });
        let color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let reevaluation = self.push_value(Value::Enum {
            value_type: "HudReeval".to_string(),
            value: "VISIBILITY_SORT_ORDER_STRING_AND_COLOR".to_string(),
        });
        let visibility = self.push_value(Value::Enum {
            value_type: "SpecVisibility".to_string(),
            value: "DEFAULT".to_string(),
        });
        Ok(self.wir.actions.push(Action::Call {
            name: "createHudText".to_string(),
            args: vec![
                all_players,
                null_value,
                text,
                null_value_2,
                hud_position,
                sort_order,
                null_value_3,
                color,
                null_value_4,
                reevaluation,
                visibility,
            ],
            span: self.wir_span(span)?,
        }))
    }

    fn lower_print(
        &mut self,
        expr: &Expr,
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
        macro_rules! call {
            ($name:literal $(, $arg:expr)* $(,)?) => {{
                let args = vec![$($arg),*];
                self.push_call($name, args)
            }};
        }

        let message = self.lower_text_value(expr)?;
        let padding_text = self.push_value(Value::String(" ".repeat(45)));
        let padding = self.push_call("customString", vec![padding_text]);
        let body_text = self.push_value(Value::String(format!("{}{{0}}", " ".repeat(125))));
        let body = self.push_call("customString", vec![body_text, padding]);
        let all_teams = self.push_value(Value::Enum {
            value_type: "Team".to_string(),
            value: "ALL".to_string(),
        });
        let all_players = call!("allPlayers", all_teams);
        let null_value = self.push_value(Value::Null);
        let null_value_2 = self.push_value(Value::Null);
        let null_value_3 = self.push_value(Value::Null);
        let hud_position = self.push_value(Value::Enum {
            value_type: "HudPosition".to_string(),
            value: "LEFT".to_string(),
        });
        let sort_order = self.push_value(Value::Number {
            value: -9999.0,
            text: "-9999".to_string(),
        });
        let color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "ORANGE".to_string(),
        });
        let reevaluation = self.push_value(Value::Enum {
            value_type: "HudReeval".to_string(),
            value: "VISIBILITY_AND_STRING".to_string(),
        });
        let visibility = self.push_value(Value::Enum {
            value_type: "SpecVisibility".to_string(),
            value: "DEFAULT".to_string(),
        });
        Ok(self.wir.actions.push(Action::Call {
            name: "createHudText".to_string(),
            args: vec![
                all_players,
                message,
                body,
                null_value,
                hud_position,
                sort_order,
                color,
                null_value_2,
                null_value_3,
                reevaluation,
                visibility,
            ],
            span: self.wir_span(span)?,
        }))
    }

    fn lower_debug_array_text(&mut self, value: wir::ValueId) -> wir::ValueId {
        macro_rules! call {
            ($name:literal $(, $arg:expr)* $(,)?) => {{
                let args = vec![$($arg),*];
                self.push_call($name, args)
            }};
        }

        let current_count = call!("countOf", call!("currentArrayElement"));
        let is_single = call!(
            "==",
            call!("countOf", call!("currentArrayElement")),
            self.push_number(1.0, "1")
        );
        let is_empty = call!("==", call!("currentArrayElement"), call!("emptyArray"));
        let not_null = call!(
            "!=",
            call!("currentArrayElement"),
            self.push_value(Value::Null)
        );
        let has_empty_array = call!("and", is_empty, not_null);
        let brackets = call!("or", is_single, has_empty_array);
        let first_element = call!(
            "customString",
            self.push_value(Value::String("[{0}]".to_string())),
            call!("currentArrayElement"),
        );
        let many_elements = call!(
            "customString",
            self.push_value(Value::String("[{0}, …+{1}]".to_string())),
            call!("currentArrayElement"),
            call!(
                "subtract",
                call!("countOf", call!("currentArrayElement")),
                self.push_number(1.0, "1"),
            ),
        );
        let element_text = call!(
            "ifThenElse",
            brackets,
            first_element,
            call!(
                "ifThenElse",
                current_count,
                many_elements,
                call!("currentArrayElement"),
            ),
        );
        let mapped_elements = call!("mappedArray", value, element_text,);
        let mapped_input = call!("array", mapped_elements);
        let current_array = call!("currentArrayElement");
        let actual_array = call!(
            "or",
            call!("countOf", current_array),
            call!(
                "and",
                call!("==", call!("currentArrayElement"), call!("emptyArray")),
                call!(
                    "!=",
                    call!("currentArrayElement"),
                    self.push_value(Value::Null)
                ),
            ),
        );
        let empty_length = call!(
            "ifThenElse",
            call!(
                "and",
                call!("not", call!("countOf", call!("currentArrayElement"))),
                call!("!=", call!("currentArrayElement"), call!("emptyArray"),),
            ),
            self.push_number(3.0, "3"),
            call!(
                "multiply",
                call!("countOf", call!("currentArrayElement")),
                self.push_number(3.0, "3"),
            ),
        );
        let x = call!(
            "appendToArray",
            call!("appendToArray", actual_array, empty_length),
            current_array,
        );
        let x_input = call!("mappedArray", mapped_input, x);
        let x_length = |this: &mut Self| {
            let current = this.push_call("currentArrayElement", Vec::new());
            let index = this.push_number(1.0, "1");
            this.push_call("valueInArray", vec![current, index])
        };
        let x_value = |this: &mut Self, index: f64| {
            let current = this.push_call("currentArrayElement", Vec::new());
            let index_value = this.push_number(index, &index.to_string());
            this.push_call("valueInArray", vec![current, index_value])
        };
        let first = call!("firstOf", call!("currentArrayElement"));
        let array_tail = call!(
            "customString",
            self.push_value(Value::String("{0}, {1}, {2}".to_string())),
            x_value(self, 4.0),
            x_value(self, 5.0),
            call!(
                "customString",
                self.push_value(Value::String("{0}, {1}, …\u{0001}".to_string())),
                x_value(self, 6.0),
                x_value(self, 7.0),
            ),
        );
        let array_head = call!(
            "customString",
            self.push_value(Value::String("{0}, {1}, {2}".to_string())),
            x_value(self, 2.0),
            x_value(self, 3.0),
            array_tail,
        );
        let placeholder = call!(
            "customString",
            self.push_value(Value::String("0, 0, 0, 0, 0, 0, …\u{0001}".to_string())),
        );
        let length_for_slice = x_length(self);
        let end_length_for_slice = x_length(self);
        let slice = call!(
            "stringSlice",
            placeholder,
            call!("add", self.push_number(-2.0, "-2"), length_for_slice),
            call!(
                "subtract",
                self.push_number(22.0, "22"),
                end_length_for_slice,
            ),
        );
        let replaced = call!("stringReplace", array_head, slice, call!("emptyArray"),);
        let length_for_compare = x_length(self);
        let length_for_divide = x_length(self);
        let plus = call!(
            "ifThenElse",
            call!(">", length_for_compare, self.push_number(18.0, "18")),
            call!(
                "customString",
                self.push_value(Value::String("+{0}".to_string())),
                call!(
                    "subtract",
                    call!("divide", length_for_divide, self.push_number(3.0, "3")),
                    self.push_number(6.0, "6"),
                ),
            ),
            call!("emptyArray"),
        );
        let formatted_array = call!(
            "customString",
            self.push_value(Value::String("[{0}{1}]".to_string())),
            replaced,
            plus,
        );
        let current_for_split = call!("currentArrayElement");
        let rendered = call!(
            "ifThenElse",
            first,
            formatted_array,
            call!(
                "stringSplit",
                call!(
                    "valueInArray",
                    current_for_split,
                    self.push_number(2.0, "2")
                ),
                call!("emptyArray"),
            ),
        );
        call!("mappedArray", x_input, rendered)
    }

    fn lower_text_value(&mut self, expr: &Expr) -> Result<wir::ValueId, IntegrationError> {
        let value = self.lower_value(expr)?;
        let Value::Call { name, args } = &self
            .wir
            .values
            .get(value)
            .expect("lowered text value must exist")
            .value
        else {
            return Ok(value);
        };
        if name == "customString" && args.len() == 1 {
            Ok(args[0])
        } else {
            Ok(value)
        }
    }

    fn debug_value_is_array(&self, value: wir::ValueId) -> bool {
        match &self
            .wir
            .values
            .get(value)
            .expect("lowered value must exist")
            .value
        {
            Value::GlobalVariable(_) | Value::Array(_) => true,
            Value::Call { name, .. } if matches!(name.as_str(), "array" | "emptyArray") => true,
            Value::Call { name, .. } => self
                .compiler
                .catalog
                .entry(Kind::Value, name)
                .and_then(|entry| entry.return_type())
                .is_some_and(|return_type| {
                    return_type.split('|').any(|part| part.trim() == "Array")
                }),
            _ => false,
        }
    }

    fn push_value(&mut self, value: Value) -> wir::ValueId {
        self.wir.values.push(ValueNode::new(value, None))
    }

    fn push_call(&mut self, name: &str, args: Vec<wir::ValueId>) -> wir::ValueId {
        self.push_value(Value::Call {
            name: name.to_string(),
            args,
        })
    }

    fn lower_custom_string(
        &mut self,
        value: String,
        span: Option<HirSpan>,
    ) -> Result<wir::ValueId, IntegrationError> {
        let span = self.wir_span(span)?;
        let text = self
            .wir
            .values
            .push(ValueNode::new(Value::String(value), span));
        Ok(self.wir.values.push(ValueNode::new(
            Value::Call {
                name: "customString".to_string(),
                args: vec![text],
            },
            span,
        )))
    }

    fn push_number(&mut self, value: f64, text: &str) -> wir::ValueId {
        self.push_value(Value::Number {
            value,
            text: text.to_string(),
        })
    }

    fn fold_numeric_binary(
        &self,
        op: &str,
        left: wir::ValueId,
        right: wir::ValueId,
    ) -> Option<f64> {
        let number = |id| match self.wir.values.get(id)?.value {
            Value::Number { value, .. } => Some(value),
            _ => None,
        };
        let left = number(left)?;
        let right = number(right)?;
        let value = match op {
            "+" => left + right,
            "-" => left - right,
            "*" => left * right,
            "/" if right != 0.0 => left / right,
            "%" if right != 0.0 => left % right,
            "**" => left.powf(right),
            _ => return None,
        };
        value.is_finite().then_some(value)
    }

    fn lower_condition(&mut self, expr: &Expr) -> Result<wir::ValueId, IntegrationError> {
        let value = self.lower_value(expr)?;
        let is_comparison = |expr: &Expr| matches!(expr, Expr::Binary { op, .. } if matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">="));
        if is_comparison(expr)
            || matches!(expr, Expr::Unary { op, operand, .. } if op == "not" && is_comparison(operand))
        {
            return Ok(value);
        }
        let true_value = self.wir.values.push(ValueNode::new(
            Value::Bool(true),
            self.wir_span(expr.span().copied())?,
        ));
        Ok(self.wir.values.push(ValueNode::new(
            Value::Call {
                name: "==".to_string(),
                args: vec![value, true_value],
            },
            self.wir_span(expr.span().copied())?,
        )))
    }

    fn lower_assign(
        &mut self,
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
        let mut indices = Vec::new();
        if let Some(root) = indexed_target_parts(target, &mut indices) {
            if indices.len() > 3 {
                return Err(self.unsupported("Cannot assign to 4d array", target.span().copied()));
            }
            if indices.len() > 1 {
                indices.reverse();
                return self.lower_nested_indexed_assign(root, &indices, target, value, span);
            }
        }
        match target {
            Expr::GlobalVar {
                name,
                span: target_span,
            } => {
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                if let Expr::Binary {
                    op, left, right, ..
                } = value
                {
                    if let Expr::GlobalVar {
                        name: left_name, ..
                    } = left.as_ref()
                    {
                        if left_name == name {
                            if let Some(modify_op) = modify_op_from_str(op) {
                                let val = self.lower_value(right)?;
                                return Ok(self.wir.actions.push(Action::ModifyGlobalVariable {
                                    variable,
                                    op: modify_op,
                                    value: val,
                                    span: self.wir_span(span)?,
                                    target_span: self.wir_span(*target_span)?,
                                }));
                            }
                        }
                    }
                }
                let val = self.lower_value(value)?;
                Ok(self.wir.actions.push(Action::SetGlobalVariable {
                    variable,
                    value: val,
                    span: self.wir_span(span)?,
                    target_span: self.wir_span(*target_span)?,
                }))
            }
            Expr::PlayerVar {
                player,
                name,
                span: target_span,
                ..
            } => {
                let variable = *self.players.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown player variable '{name}'"), *target_span)
                })?;
                let player_val = self.lower_value(player)?;
                if let Expr::Binary {
                    op, left, right, ..
                } = value
                {
                    if let Expr::PlayerVar {
                        player: left_player,
                        name: left_name,
                        ..
                    } = left.as_ref()
                    {
                        if left_name == name && left_player.as_ref() == player.as_ref() {
                            if let Some(modify_op) = modify_op_from_str(op) {
                                let val = self.lower_value(right)?;
                                return Ok(self.wir.actions.push(Action::ModifyPlayerVariable {
                                    player: player_val,
                                    variable,
                                    op: modify_op,
                                    value: val,
                                    span: self.wir_span(span)?,
                                    target_span: self.wir_span(*target_span)?,
                                }));
                            }
                        }
                    }
                }
                let val = self.lower_value(value)?;
                Ok(self.wir.actions.push(Action::SetPlayerVariable {
                    player: player_val,
                    variable,
                    value: val,
                    span: self.wir_span(span)?,
                    target_span: self.wir_span(*target_span)?,
                }))
            }
            Expr::Index {
                array,
                index,
                span: target_span,
            } => match array.as_ref() {
                Expr::GlobalVar {
                    name,
                    span: arr_span,
                } => {
                    let variable = *self.globals.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown global variable '{name}'"), *arr_span)
                    })?;
                    let var_node = self.wir.values.push(ValueNode::new(
                        Value::GlobalVariable(variable),
                        self.wir_span(*arr_span)?,
                    ));
                    let index_val = self.lower_value(index)?;
                    if let Expr::Binary {
                        op, left, right, ..
                    } = value
                    {
                        if let Expr::Index {
                            array: left_arr,
                            index: left_idx,
                            ..
                        } = left.as_ref()
                        {
                            if left_arr.as_ref() == array.as_ref()
                                && left_idx.as_ref() == index.as_ref()
                            {
                                if let Some(op_id) = modify_catalog_name_from_str(op) {
                                    let op_node = self.wir.values.push(ValueNode::new(
                                        Value::Call {
                                            name: op_id.to_string(),
                                            args: Vec::new(),
                                        },
                                        None,
                                    ));
                                    let right_val = self.lower_value(right)?;
                                    return Ok(self.wir.actions.push(Action::Call {
                                        name: "modifyGlobalVariableAtIndex".to_string(),
                                        args: vec![var_node, index_val, op_node, right_val],
                                        span: self.wir_span(span)?,
                                    }));
                                }
                            }
                        }
                    }
                    let val = self.lower_value(value)?;
                    Ok(self.wir.actions.push(Action::Call {
                        name: "setGlobalVariableAtIndex".to_string(),
                        args: vec![var_node, index_val, val],
                        span: self.wir_span(span)?,
                    }))
                }
                Expr::PlayerVar {
                    player,
                    name,
                    span: arr_span,
                    ..
                } => {
                    let player_val = self.lower_value(player)?;
                    let variable = *self.players.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown player variable '{name}'"), *arr_span)
                    })?;
                    let var_node = self.wir.values.push(ValueNode::new(
                        Value::PlayerVariable {
                            player: player_val,
                            variable,
                        },
                        self.wir_span(*arr_span)?,
                    ));
                    let index_val = self.lower_value(index)?;
                    if let Expr::Binary {
                        op, left, right, ..
                    } = value
                    {
                        if let Expr::Index {
                            array: left_arr,
                            index: left_idx,
                            ..
                        } = left.as_ref()
                        {
                            if left_arr.as_ref() == array.as_ref()
                                && left_idx.as_ref() == index.as_ref()
                            {
                                if let Some(op_id) = modify_catalog_name_from_str(op) {
                                    let op_node = self.wir.values.push(ValueNode::new(
                                        Value::Call {
                                            name: op_id.to_string(),
                                            args: Vec::new(),
                                        },
                                        None,
                                    ));
                                    let right_val = self.lower_value(right)?;
                                    // The canonical signature takes the
                                    // player-variable value node (which
                                    // carries the player) as its first
                                    // argument.
                                    return Ok(self.wir.actions.push(Action::Call {
                                        name: "modifyPlayerVariableAtIndex".to_string(),
                                        args: vec![var_node, index_val, op_node, right_val],
                                        span: self.wir_span(span)?,
                                    }));
                                }
                            }
                        }
                    }
                    let val = self.lower_value(value)?;
                    // The canonical signature takes the player-variable
                    // value node (which carries the player) as its first
                    // argument.
                    Ok(self.wir.actions.push(Action::Call {
                        name: "setPlayerVariableAtIndex".to_string(),
                        args: vec![var_node, index_val, val],
                        span: self.wir_span(span)?,
                    }))
                }
                _ => Err(self.unsupported(
                    "indexing assignment is only representable for global or player variables",
                    *target_span,
                )),
            },
            _ => Err(self.unsupported(
                "only global-variable, player-variable, or index assignment is currently representable in canonical WIR",
                span,
            )),
        }
    }

    fn lower_nested_indexed_assign(
        &mut self,
        root: &Expr,
        indices: &[&Expr],
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
        let (action_name, root_value) = match root {
            Expr::GlobalVar {
                name,
                span: target_span,
            } => {
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                let root_value = self.wir.values.push(ValueNode::new(
                    Value::GlobalVariable(variable),
                    self.wir_span(*target_span)?,
                ));
                ("setGlobalVariableAtIndex", root_value)
            }
            Expr::PlayerVar {
                player,
                name,
                span: target_span,
                ..
            } => {
                let player_value = self.lower_value(player)?;
                let variable = *self.players.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown player variable '{name}'"), *target_span)
                })?;
                let root_value = self.wir.values.push(ValueNode::new(
                    Value::PlayerVariable {
                        player: player_value,
                        variable,
                    },
                    self.wir_span(*target_span)?,
                ));
                ("setPlayerVariableAtIndex", root_value)
            }
            _ => {
                return Err(self.unsupported(
                    "indexing assignment is only representable for global or player variables",
                    target.span().copied(),
                ));
            }
        };

        let outer_index = self.lower_value(indices[0])?;
        let outer_array = self.lower_indexed_read(root_value, indices[0], outer_index)?;
        let replacement =
            self.rebuild_indexed_value(outer_array, &indices[1..], target, value, span)?;
        Ok(self.wir.actions.push(Action::Call {
            name: action_name.to_string(),
            args: vec![root_value, outer_index, replacement],
            span: self.wir_span(span)?,
        }))
    }

    fn rebuild_indexed_value(
        &mut self,
        array: wir::ValueId,
        indices: &[&Expr],
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<wir::ValueId, IntegrationError> {
        let index = indices
            .first()
            .copied()
            .expect("nested indexed assignment has an inner index");
        let index_value = self.lower_value(index)?;
        let replacement = if indices.len() == 1 {
            if let Expr::Binary {
                op, left, right, ..
            } = value
                && left.as_ref() == target
                && let Some(call_name) = modify_catalog_name_from_str(op)
            {
                let current = self.lower_indexed_read(array, index, index_value)?;
                let right = self.lower_value(right)?;
                self.push_call(call_name, vec![current, right])
            } else {
                self.lower_value(value)?
            }
        } else {
            let child = self.lower_indexed_read(array, index, index_value)?;
            self.rebuild_indexed_value(child, &indices[1..], target, value, span)?
        };
        self.replace_array_element(array, index_value, replacement, span)
    }

    fn lower_indexed_read(
        &mut self,
        array: wir::ValueId,
        index: &Expr,
        index_value: wir::ValueId,
    ) -> Result<wir::ValueId, IntegrationError> {
        if matches!(index, Expr::Number { value, .. } if *value == 0.0) {
            Ok(self.push_call("firstOf", vec![array]))
        } else {
            Ok(self.push_call("valueInArray", vec![array, index_value]))
        }
    }

    fn replace_array_element(
        &mut self,
        array: wir::ValueId,
        index: wir::ValueId,
        replacement: wir::ValueId,
        span: Option<HirSpan>,
    ) -> Result<wir::ValueId, IntegrationError> {
        let zero = self.push_number(0.0, "0");
        let one = self.push_number(1.0, "1");
        let end = self.push_call("add", vec![index, one]);
        let maximum = self.push_number(999_999_999_999.0, "999999999999");
        let prefix = self.push_call("slice", vec![array, zero, index]);
        let middle = self.lower_array(vec![replacement], span)?;
        let suffix = self.push_call("slice", vec![array, end, maximum]);
        let with_replacement = self.push_call("appendToArray", vec![prefix, middle]);
        Ok(self.push_call("appendToArray", vec![with_replacement, suffix]))
    }

    fn lower_action_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
        if name == "chaseAtRate" {
            let args = args
                .iter()
                .map(|expr| self.lower_value(expr))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(self.wir.actions.push(Action::Call {
                name: name.to_string(),
                args,
                span: self.wir_span(span)?,
            }));
        }
        let function = self
            .compiler
            .manifest
            .resolve_function(name)
            .ok_or_else(|| self.unsupported(format!("unknown action '{name}'"), span))?;
        if !matches!(function.kind, FunctionKind::Action) {
            return Err(self.unsupported(format!("'{name}' is not a generic OPY action"), span));
        }
        if matches!(
            function.id.as_str(),
            "hudHeader" | "hudSubheader" | "hudSubtext"
        ) {
            let text_slot = match function.id.as_str() {
                "hudHeader" => 1,
                "hudSubheader" => 2,
                "hudSubtext" => 3,
                _ => unreachable!(),
            };
            return self.lower_hud_text(args, span, text_slot, &function.id);
        }
        if function.id == "createDummy" && args.len() == 4 {
            let mut lowered = args
                .iter()
                .map(|expr| self.lower_value(expr))
                .collect::<Result<Vec<_>, _>>()?;
            let mut zero_vector = Vec::with_capacity(3);
            for value in [0.0, 0.0, 0.0] {
                zero_vector.push(self.push_value(Value::Number {
                    value,
                    text: "0".to_string(),
                }));
            }
            lowered.push(self.push_call("vector", zero_vector));
            return Ok(self.wir.actions.push(Action::Call {
                name: "createDummyBot".to_string(),
                args: lowered,
                span: self.wir_span(span)?,
            }));
        }
        let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
            self.unsupported(
                format!(
                    "action '{}' requires a special lowering not in #46",
                    function.id
                ),
                span,
            )
        })?;
        let args = args
            .iter()
            .map(|expr| self.lower_value(expr))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.wir.actions.push(Action::Call {
            name: catalog_id.clone(),
            args,
            span: self.wir_span(span)?,
        }))
    }

    fn lower_hud_text(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
        text_slot: usize,
        function_name: &str,
    ) -> Result<wir::ActionId, IntegrationError> {
        let [
            visible_to,
            text,
            position,
            sort_order,
            color,
            reevaluation,
            spectators,
        ] = args
        else {
            return Err(self.unsupported(
                format!("{function_name} requires exactly seven bound arguments"),
                span,
            ));
        };
        let visible_to = self.lower_hud_visible_to(visible_to)?;
        let mut text_slots = [
            self.push_value(Value::Null),
            self.push_value(Value::Null),
            self.push_value(Value::Null),
        ];
        let text_value = self.lower_text_value(text)?;
        text_slots[text_slot - 1] = self.push_call("customString", vec![text_value]);
        let mut colors = [
            self.push_value(Value::Null),
            self.push_value(Value::Null),
            self.push_value(Value::Null),
        ];
        colors[text_slot - 1] = self.lower_value(color)?;
        let args = vec![
            visible_to,
            text_slots[0],
            text_slots[1],
            text_slots[2],
            self.lower_value(position)?,
            self.lower_value(sort_order)?,
            colors[0],
            colors[1],
            colors[2],
            self.lower_value(reevaluation)?,
            self.lower_value(spectators)?,
        ];
        Ok(self.wir.actions.push(Action::Call {
            name: "createHudText".to_string(),
            args,
            span: self.wir_span(span)?,
        }))
    }

    fn lower_hud_visible_to(&mut self, expr: &Expr) -> Result<wir::ValueId, IntegrationError> {
        if let Expr::Call { name, args, .. } = expr {
            if name == "getAllPlayers" && args.is_empty() {
                let all_teams = self.push_value(Value::Enum {
                    value_type: "Team".to_string(),
                    value: "ALL".to_string(),
                });
                return Ok(self.push_call("allPlayers", vec![all_teams]));
            }
        }
        self.lower_value(expr)
    }

    fn lower_receiver_action_call(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
        let function = self
            .compiler
            .manifest
            .resolve_member(name)
            .ok_or_else(|| self.unsupported(format!("unknown member action '{name}'"), span))?;
        if !matches!(function.kind, FunctionKind::MemberAction) {
            return Err(self.unsupported(format!("'{name}' is not a member action"), span));
        }

        // `append` is an OPY mutation, represented by the canonical variable
        // modify actions rather than a catalog action call.
        if function.id == "append" {
            let [value] = args else {
                return Err(self.unsupported("append requires exactly one argument", span));
            };
            let value = self.lower_value(value)?;
            return match receiver {
                Expr::GlobalVar {
                    name,
                    span: target_span,
                } => {
                    let variable = *self.globals.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                    })?;
                    Ok(self.wir.actions.push(Action::ModifyGlobalVariable {
                        variable,
                        op: wir::ModifyOp::AppendToArray,
                        value,
                        span: self.wir_span(span)?,
                        target_span: self.wir_span(*target_span)?,
                    }))
                }
                Expr::PlayerVar {
                    player,
                    name,
                    span: target_span,
                    ..
                } => {
                    let variable = *self.players.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown player variable '{name}'"), *target_span)
                    })?;
                    let player = self.lower_value(player)?;
                    Ok(self.wir.actions.push(Action::ModifyPlayerVariable {
                        player,
                        variable,
                        op: wir::ModifyOp::AppendToArray,
                        value,
                        span: self.wir_span(span)?,
                        target_span: self.wir_span(*target_span)?,
                    }))
                }
                _ => Err(self.unsupported(
                    "append requires a global or player variable receiver",
                    receiver.span().copied().or(span),
                )),
            };
        }

        let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
            self.unsupported(
                format!(
                    "member action '{}' has no canonical catalog identity",
                    function.id
                ),
                span,
            )
        })?;
        let mut lowered = Vec::with_capacity(args.len() + 1);
        lowered.push(self.lower_value(receiver)?);
        lowered.extend(
            args.iter()
                .map(|arg| self.lower_value(arg))
                .collect::<Result<Vec<_>, _>>()?,
        );
        Ok(self.wir.actions.push(Action::Call {
            name: catalog_id.clone(),
            args: lowered,
            span: self.wir_span(span)?,
        }))
    }

    fn lower_value(&mut self, expr: &Expr) -> Result<wir::ValueId, IntegrationError> {
        let span = expr.span().copied();
        let value = match expr {
            Expr::Number { value, text, .. } => Value::Number {
                value: *value,
                text: canonical_number_text(*value, text),
            },
            Expr::String { value, .. } => {
                return self.lower_custom_string(value.clone(), span);
            }
            Expr::Bool { value, .. } => Value::Bool(*value),
            Expr::Null { .. } => Value::Null,
            Expr::Local { name, .. } => {
                let binding = self.array_bindings.iter().rev().find(|binding| {
                    binding.element == *name || binding.index.as_deref() == Some(name)
                });
                match binding {
                    Some(binding) if binding.element == *name => {
                        return Ok(self.push_call("currentArrayElement", Vec::new()));
                    }
                    Some(_) => return Ok(self.push_call("currentArrayIndex", Vec::new())),
                    None => {
                        return Err(self.unsupported(
                            format!("local '{name}' is not inside a supported array callback"),
                            span,
                        ));
                    }
                }
            }
            Expr::Type { .. } => {
                return Err(self.unsupported(
                    "type expressions are only valid as createWorkshopSetting type arguments",
                    span,
                ));
            }
            Expr::GlobalVar { name, .. } => {
                let id = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), span)
                })?;
                Value::GlobalVariable(id)
            }
            Expr::PlayerVar { player, name, .. } => {
                let player = self.lower_value(player)?;
                let id = *self.players.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown player variable '{name}'"), span)
                })?;
                Value::PlayerVariable {
                    player,
                    variable: id,
                }
            }
            Expr::EventPlayer { .. } => Value::EventPlayer,
            Expr::HostPlayer { .. } => Value::Call {
                name: "hostPlayer".to_string(),
                args: Vec::new(),
            },
            Expr::Enum {
                value_type, value, ..
            } => {
                if self
                    .compiler
                    .catalog
                    .enum_spelling(value_type, &Locale::new("en-US"), value)
                    .is_none()
                {
                    return Err(self.unsupported(
                        format!("unknown catalog enum member '{value_type}.{value}'"),
                        span,
                    ));
                }
                Value::Enum {
                    value_type: value_type.clone(),
                    value: value.clone(),
                }
            }
            Expr::Array { elements, .. } => {
                let elements = elements
                    .iter()
                    .map(|element| self.lower_value(element))
                    .collect::<Result<Vec<_>, _>>()?;
                return self.lower_array(elements, span);
            }
            Expr::Vector { x, y, z, .. } => Value::Call {
                name: "vector".to_string(),
                args: vec![
                    self.lower_value(x)?,
                    self.lower_value(y)?,
                    self.lower_value(z)?,
                ],
            },
            Expr::Constant { name, .. } => {
                let const_expr = *self
                    .constants
                    .get(name)
                    .ok_or_else(|| self.unsupported(format!("unknown constant '{name}'"), span))?;
                return self.lower_value(const_expr);
            }
            Expr::Index { array, index, .. } => {
                if let Expr::Dict { entries, .. } = array.as_ref()
                    && is_literal_key(index)
                    && entries.iter().all(|entry| is_literal_key(&entry.key))
                {
                    if let Some(value) = entries
                        .iter()
                        .find(|entry| literal_key_matches(&entry.key, index))
                        .map(|entry| &entry.value)
                    {
                        return self.lower_value(value);
                    }
                    return Ok(self
                        .wir
                        .values
                        .push(ValueNode::new(Value::Null, self.wir_span(span)?)));
                }
                // The pinned OverPy oracle lowers a literal zero-index read
                // (`arr[0]`, `arr[0.0]`) to `firstOf(arr)`; non-zero indexes
                // and indexed writes keep the indexed forms.
                if matches!(index.as_ref(), Expr::Number { value, .. } if *value == 0.0) {
                    Value::Call {
                        name: "firstOf".to_string(),
                        args: vec![self.lower_value(array)?],
                    }
                } else {
                    Value::Call {
                        name: "valueInArray".to_string(),
                        args: vec![self.lower_value(array)?, self.lower_value(index)?],
                    }
                }
            }
            Expr::Format { text, args, .. } => {
                if let Some(value) = fold_literal_format(text, args) {
                    return self.lower_custom_string(value, span);
                }
                let text_node = self.wir.values.push(ValueNode::new(
                    Value::String(canonical_format_text(text)),
                    self.wir_span(span)?,
                ));
                let mut call_args = vec![text_node];
                for arg in args {
                    call_args.push(self.lower_value(arg)?);
                }
                Value::Call {
                    name: "customString".to_string(),
                    args: call_args,
                }
            }
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                ..
            } => Value::Call {
                name: "ifThenElse".to_string(),
                args: vec![
                    self.lower_value(condition)?,
                    self.lower_value(then_value)?,
                    self.lower_value(else_value)?,
                ],
            },
            Expr::Binary {
                op, left, right, ..
            } => {
                let left = self.lower_value(left)?;
                let right = self.lower_value(right)?;
                if let Some(value) = self.fold_numeric_binary(op, left, right) {
                    Value::Number {
                        value,
                        text: computed_number_text(value),
                    }
                } else {
                    match op.as_str() {
                        "==" | "!=" | "<" | "<=" | ">" | ">=" => Value::Call {
                            name: op.clone(),
                            args: vec![left, right],
                        },
                        "+" => Value::Call {
                            name: "add".to_string(),
                            args: vec![left, right],
                        },
                        "-" => Value::Call {
                            name: "subtract".to_string(),
                            args: vec![left, right],
                        },
                        "*" => Value::Call {
                            name: "multiply".to_string(),
                            args: vec![left, right],
                        },
                        "/" => Value::Call {
                            name: "divide".to_string(),
                            args: vec![left, right],
                        },
                        "%" => Value::Call {
                            name: "modulo".to_string(),
                            args: vec![left, right],
                        },
                        "**" => Value::Call {
                            name: "raiseToPower".to_string(),
                            args: vec![left, right],
                        },
                        "and" => Value::Call {
                            name: "and".to_string(),
                            args: vec![left, right],
                        },
                        "or" => Value::Call {
                            name: "or".to_string(),
                            args: vec![left, right],
                        },
                        "in" => Value::Call {
                            name: "arrayContains".to_string(),
                            args: vec![right, left],
                        },
                        "not in" => {
                            let wir_span = self.wir_span(span)?;
                            let contains = self.wir.values.push(ValueNode::new(
                                Value::Call {
                                    name: "arrayContains".to_string(),
                                    args: vec![right, left],
                                },
                                wir_span,
                            ));
                            Value::Call {
                                name: "not".to_string(),
                                args: vec![contains],
                            }
                        }
                        _ => {
                            return Err(self.unsupported(
                                format!(
                                    "binary operator '{op}' is not currently representable in canonical WIR"
                                ),
                                span,
                            ));
                        }
                    }
                }
            }
            Expr::Unary { op, operand, .. } => match op.as_str() {
                "not" => {
                    // The pinned OverPy 9.7.10 oracle lowers `not (a == b)`
                    // to the negated comparison (`a != b`), flipping every
                    // ordering comparison; `in` membership stays wrapped in
                    // `not`. Mirror that observable lowering.
                    if let Expr::Binary {
                        op: comparison,
                        left,
                        right,
                        ..
                    } = operand.as_ref()
                    {
                        if let Some(negated) = negated_comparison(comparison) {
                            Value::Call {
                                name: negated.to_string(),
                                args: vec![self.lower_value(left)?, self.lower_value(right)?],
                            }
                        } else {
                            Value::Call {
                                name: "not".to_string(),
                                args: vec![self.lower_value(operand)?],
                            }
                        }
                    } else {
                        Value::Call {
                            name: "not".to_string(),
                            args: vec![self.lower_value(operand)?],
                        }
                    }
                }
                "-" => Value::Call {
                    name: "-".to_string(),
                    args: vec![self.lower_value(operand)?],
                },
                "+" => return self.lower_value(operand),
                _ => {
                    return Err(self.unsupported(
                        format!(
                            "unary operator '{op}' is not currently representable in canonical WIR"
                        ),
                        span,
                    ));
                }
            },
            Expr::Call { name, args, .. } => {
                if name == "createWorkshopSetting" {
                    return self.lower_workshop_setting(args, span);
                }
                if matches!(name.as_str(), "attacker" | "victim") && args.is_empty() {
                    return Ok(self.push_call(name, Vec::new()));
                }
                if name == "ruleCondition" {
                    if !args.is_empty() {
                        return Err(
                            self.unsupported("ruleCondition does not accept arguments", span)
                        );
                    }
                    let conditions = self.current_rule_conditions.clone().ok_or_else(|| {
                        self.unsupported("ruleCondition is only valid inside a rule", span)
                    })?;
                    let Some((first, rest)) = conditions.split_first() else {
                        return Ok(self.push_value(Value::Bool(true)));
                    };
                    let mut combined = *first;
                    for condition in rest {
                        combined = self.push_call("and", vec![combined, *condition]);
                    }
                    return Ok(combined);
                }
                if name == "vect" && args.len() == 3 {
                    Value::Vector {
                        x: self.lower_value(&args[0])?,
                        y: self.lower_value(&args[1])?,
                        z: self.lower_value(&args[2])?,
                    }
                } else if matches!(name.as_str(), "all" | "any") {
                    let call_name = if name == "all" {
                        "isTrueForAll"
                    } else {
                        "isTrueForAny"
                    };
                    let [array] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{name} requires exactly one array argument"),
                            span,
                        ));
                    };
                    let (array, condition) = match array {
                        Expr::Comprehension {
                            element,
                            variable,
                            index,
                            iterable,
                            ..
                        } => {
                            if index.is_some() {
                                return Err(self.unsupported(
                                    format!("{name} does not support an index binder"),
                                    span,
                                ));
                            }
                            let iterable = self.lower_value(iterable)?;
                            self.array_bindings.push(ArrayBinding {
                                element: variable.clone(),
                                index: None,
                            });
                            let condition = self.lower_value(element);
                            self.array_bindings.pop();
                            (iterable, condition?)
                        }
                        array => (
                            self.lower_value(array)?,
                            self.push_call("currentArrayElement", Vec::new()),
                        ),
                    };
                    Value::Call {
                        name: call_name.to_string(),
                        args: vec![array, condition],
                    }
                } else if matches!(name.as_str(), "ceil" | "floor" | "round") {
                    let [value] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{name} requires exactly one numeric argument"),
                            span,
                        ));
                    };
                    let rounding = match name.as_str() {
                        "ceil" => "UP",
                        "floor" => "DOWN",
                        "round" => "NEAREST",
                        _ => unreachable!(),
                    };
                    let rounding = self.push_value(Value::Enum {
                        value_type: "Rounding".to_string(),
                        value: rounding.to_string(),
                    });
                    Value::Call {
                        name: "roundToInteger".to_string(),
                        args: vec![self.lower_value(value)?, rounding],
                    }
                } else if name == "sorted" {
                    let (array, key) = match args.as_slice() {
                        [array] => (
                            self.lower_value(array)?,
                            self.push_call("currentArrayElement", Vec::new()),
                        ),
                        [
                            array,
                            Expr::Lambda {
                                params, body, span, ..
                            },
                        ] => {
                            let array = self.lower_value(array)?;
                            let key = self.lower_array_callback(params, body, *span)?;
                            (array, key)
                        }
                        _ => {
                            return Err(self.unsupported(
                                "sorted requires an array and an optional lambda key",
                                span,
                            ));
                        }
                    };
                    Value::Call {
                        name: "sortedArray".to_string(),
                        args: vec![array, key],
                    }
                } else {
                    let function = self
                        .compiler
                        .manifest
                        .resolve_function(name)
                        .ok_or_else(|| self.unsupported(format!("unknown value '{name}'"), span))?;
                    if !matches!(function.kind, FunctionKind::Value) {
                        return Err(
                            self.unsupported(format!("'{name}' is not a generic OPY value"), span)
                        );
                    }
                    let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
                        self.unsupported(
                            format!(
                                "value '{}' requires a special lowering not in #46",
                                function.id
                            ),
                            span,
                        )
                    })?;
                    Value::Call {
                        name: catalog_id.clone(),
                        args: args
                            .iter()
                            .map(|arg| self.lower_value(arg))
                            .collect::<Result<Vec<_>, _>>()?,
                    }
                }
            }
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => {
                let function = self.compiler.manifest.resolve_member(name).ok_or_else(|| {
                    self.unsupported(format!("unknown member value '{name}'"), span)
                })?;
                if !matches!(function.kind, FunctionKind::MemberValue) {
                    return Err(self.unsupported(format!("'{name}' is not a member value"), span));
                }
                let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
                    self.unsupported(
                        format!(
                            "member value '{}' has no canonical catalog identity",
                            function.id
                        ),
                        span,
                    )
                })?;
                let mut lowered = Vec::with_capacity(args.len() + 1);
                lowered.push(self.lower_value(receiver)?);
                lowered.extend(
                    args.iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                Value::Call {
                    name: catalog_id.clone(),
                    args: lowered,
                }
            }
            Expr::Member {
                receiver, member, ..
            } => {
                let receiver = self.lower_value(receiver)?;
                let member = self.wir.values.push(ValueNode::new(
                    Value::String(member.clone()),
                    self.wir_span(span)?,
                ));
                Value::Call {
                    name: "memberAccess".to_string(),
                    args: vec![receiver, member],
                }
            }
            Expr::Comprehension {
                element,
                variable,
                index,
                iterable,
                condition,
                span: comprehension_span,
                ..
            } => {
                if condition.is_some() && index.is_some() {
                    return Err(self.unsupported(
                        "comprehensions with both a filter and an index binder are not currently representable in canonical WIR",
                        *comprehension_span,
                    ));
                }
                let iterable = self.lower_value(iterable)?;
                let binding = ArrayBinding {
                    element: variable.clone(),
                    index: index.clone(),
                };
                self.array_bindings.push(binding);
                let predicate = condition
                    .as_deref()
                    .map(|condition| self.lower_value(condition));
                let element = self.lower_value(element);
                self.array_bindings.pop();
                let element = element?;
                let iterable = if let Some(predicate) = predicate {
                    let predicate = predicate?;
                    self.push_call("filteredArray", vec![iterable, predicate])
                } else {
                    iterable
                };
                Value::Call {
                    name: "mappedArray".to_string(),
                    args: vec![iterable, element],
                }
            }
            Expr::Lambda { span, .. } => {
                return Err(self.unsupported(
                    "lambda expressions are only representable as supported array operation arguments",
                    *span,
                ));
            }
            Expr::StringModifier {
                modifier,
                value,
                span,
            } => {
                let value = match modifier.as_str() {
                    "b" => big_letters(value),
                    "c" => case_sensitive(value),
                    "w" => fullwidth(value),
                    _ => {
                        return Err(self.unsupported(
                            format!(
                                "string modifier '{modifier}' is not currently representable in canonical WIR"
                            ),
                            *span,
                        ));
                    }
                };
                return self.lower_custom_string(value, *span);
            }
            _ => {
                return Err(self.unsupported(
                    format!(
                        "expression '{}' is not currently representable in canonical WIR",
                        expr.kind_name()
                    ),
                    span,
                ));
            }
        };
        Ok(self
            .wir
            .values
            .push(ValueNode::new(value, self.wir_span(span)?)))
    }

    fn lower_array_callback(
        &mut self,
        params: &[String],
        body: &Expr,
        span: Option<HirSpan>,
    ) -> Result<wir::ValueId, IntegrationError> {
        if !(1..=2).contains(&params.len()) {
            return Err(self.unsupported(
                "array callbacks require one element parameter and at most one index parameter",
                span,
            ));
        }
        if params.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(
                self.unsupported("array callback parameters must have distinct names", span)
            );
        }
        self.array_bindings.push(ArrayBinding {
            element: params[0].clone(),
            index: params.get(1).cloned(),
        });
        let result = self.lower_value(body);
        self.array_bindings.pop();
        result
    }

    fn lower_workshop_setting(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<wir::ValueId, IntegrationError> {
        let [
            Expr::Type {
                name: setting_type,
                args: type_args,
                span: type_span,
            },
            category,
            setting_name,
            default,
            sort_order,
        ] = args
        else {
            return Err(self.unsupported(
                "createWorkshopSetting requires a type and four value arguments",
                span,
            ));
        };

        let catalog_name = match (setting_type.as_str(), type_args.as_slice()) {
            ("bool", []) => "createWorkshopSettingBool",
            ("int", [_, _]) => "createWorkshopSettingInt",
            ("float", [_, _]) => "createWorkshopSettingFloat",
            ("int", []) | ("float", []) => {
                return Err(self.unsupported(
                    format!("createWorkshopSetting type '{setting_type}' requires a numeric range"),
                    type_span.or(span),
                ));
            }
            _ => {
                return Err(self.unsupported(
                    format!("unsupported createWorkshopSetting type '{setting_type}'"),
                    type_span.or(span),
                ));
            }
        };

        // OverPy uses an ideographic space for an empty setting category so
        // the generated Workshop setting has a non-empty category value.
        let category = match category {
            Expr::String { value, .. } if value.is_empty() => {
                self.push_value(Value::String("\u{3000}".to_string()))
            }
            _ => self.lower_value(category)?,
        };
        let mut lowered = vec![
            category,
            self.lower_value(setting_name)?,
            self.lower_value(default)?,
        ];
        if let [minimum, maximum] = type_args.as_slice() {
            lowered.push(self.lower_value(minimum)?);
            lowered.push(self.lower_value(maximum)?);
        }
        lowered.push(self.lower_value(sort_order)?);
        Ok(self.push_call(catalog_name, lowered))
    }

    fn wir_span(&self, span: Option<HirSpan>) -> Result<Option<WorkshopSpan>, IntegrationError> {
        let Some(span) = span else {
            return Ok(None);
        };
        let file = *self.files.get(&span.file).ok_or_else(|| {
            IntegrationError::new(
                "source-file",
                format!("HIR span references unknown source file id {}", span.file),
                Some(span),
            )
        })?;
        Ok(Some(WorkshopSpan::new(
            file,
            WorkshopPosition::new(span.start.line, span.start.col),
            WorkshopPosition::new(span.end.line, span.end.col),
        )))
    }

    fn hir_span_from_workshop(&self, span: WorkshopSpan) -> Option<HirSpan> {
        let file = *self.wir_to_hir_files.get(span.file.index())?;
        Some(HirSpan {
            file,
            start: hir::Position {
                line: span.start.line,
                col: span.start.col,
            },
            end: hir::Position {
                line: span.end.line,
                col: span.end.col,
            },
        })
    }

    fn unsupported(&self, message: impl Into<String>, span: Option<HirSpan>) -> IntegrationError {
        IntegrationError::new("unsupported-integration-surface", message, span)
    }
}

/// Collect the pinned OverPy implicit default global and player variables.
/// Global and player namespaces each have independent fixed Workshop slots;
/// only `eventPlayer.<name>` creates an implicit player variable.
fn implicit_default_variables(
    hir: &hir::Program,
) -> (
    BTreeMap<String, Option<HirSpan>>,
    BTreeMap<String, Option<HirSpan>>,
) {
    let declared_globals = hir
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            hir::Declaration::GlobalVariable { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let declared_players = hir
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            hir::Declaration::PlayerVariable { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut globals = BTreeMap::new();
    let mut players = BTreeMap::new();
    for declaration in &hir.declarations {
        let initializer = match declaration {
            hir::Declaration::GlobalVariable { initializer, .. }
            | hir::Declaration::PlayerVariable { initializer, .. } => initializer.as_ref(),
            hir::Declaration::Constant { value, .. } => Some(value),
            _ => None,
        };
        if let Some(expr) = initializer {
            collect_implicit_expr(
                expr,
                &declared_globals,
                &declared_players,
                &mut globals,
                &mut players,
            );
        }
    }
    for entry in &hir.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for condition in &rule.conditions {
                    collect_implicit_expr(
                        condition,
                        &declared_globals,
                        &declared_players,
                        &mut globals,
                        &mut players,
                    );
                }
                collect_implicit_stmts(
                    &rule.actions,
                    &declared_globals,
                    &declared_players,
                    &mut globals,
                    &mut players,
                );
            }
            RuleEntry::SubroutineDef { body, .. } => collect_implicit_stmts(
                body,
                &declared_globals,
                &declared_players,
                &mut globals,
                &mut players,
            ),
        }
    }
    (globals, players)
}

fn collect_implicit_stmts(
    statements: &[Stmt],
    declared_globals: &HashSet<&str>,
    declared_players: &HashSet<&str>,
    globals: &mut BTreeMap<String, Option<HirSpan>>,
    players: &mut BTreeMap<String, Option<HirSpan>>,
) {
    for statement in statements {
        match statement {
            Stmt::Expr { expr, .. } => {
                collect_implicit_expr(expr, declared_globals, declared_players, globals, players)
            }
            Stmt::Assign { target, value, .. } => {
                collect_implicit_expr(target, declared_globals, declared_players, globals, players);
                collect_implicit_expr(value, declared_globals, declared_players, globals, players);
            }
            Stmt::Delete { target, .. } => {
                collect_implicit_expr(target, declared_globals, declared_players, globals, players);
            }
            Stmt::If {
                branches, r#else, ..
            } => {
                for branch in branches {
                    collect_implicit_expr(
                        &branch.condition,
                        declared_globals,
                        declared_players,
                        globals,
                        players,
                    );
                    collect_implicit_stmts(
                        &branch.body,
                        declared_globals,
                        declared_players,
                        globals,
                        players,
                    );
                }
                if let Some(default_body) = r#else {
                    collect_implicit_stmts(
                        default_body,
                        declared_globals,
                        declared_players,
                        globals,
                        players,
                    );
                }
            }
            Stmt::For {
                variable,
                iterable,
                body,
                ..
            } => {
                collect_implicit_expr(
                    variable,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
                collect_implicit_expr(
                    iterable,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
                collect_implicit_stmts(body, declared_globals, declared_players, globals, players);
            }
            Stmt::While {
                condition, body, ..
            }
            | Stmt::DoWhile {
                condition, body, ..
            } => {
                collect_implicit_expr(
                    condition,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
                collect_implicit_stmts(body, declared_globals, declared_players, globals, players);
            }
            Stmt::Switch { value, arms, .. } => {
                collect_implicit_expr(value, declared_globals, declared_players, globals, players);
                for arm in arms {
                    match arm {
                        SwitchArm::Case { value, body, .. } => {
                            collect_implicit_expr(
                                value,
                                declared_globals,
                                declared_players,
                                globals,
                                players,
                            );
                            collect_implicit_stmts(
                                body,
                                declared_globals,
                                declared_players,
                                globals,
                                players,
                            );
                        }
                        SwitchArm::Default { body, .. } => {
                            collect_implicit_stmts(
                                body,
                                declared_globals,
                                declared_players,
                                globals,
                                players,
                            );
                        }
                    }
                }
            }
            Stmt::Goto { offset, .. } => {
                if let Some(offset) = offset {
                    collect_implicit_expr(
                        offset,
                        declared_globals,
                        declared_players,
                        globals,
                        players,
                    );
                }
            }
            Stmt::Break { .. }
            | Stmt::Continue { .. }
            | Stmt::Label { .. }
            | Stmt::CallSubroutine { .. }
            | Stmt::Pass { .. } => {}
        }
    }
}

fn collect_implicit_expr(
    expr: &Expr,
    declared_globals: &HashSet<&str>,
    declared_players: &HashSet<&str>,
    globals: &mut BTreeMap<String, Option<HirSpan>>,
    players: &mut BTreeMap<String, Option<HirSpan>>,
) {
    match expr {
        Expr::GlobalVar { name, span } => {
            if !declared_globals.contains(name.as_str()) && default_var_index(name).is_some() {
                globals.entry(name.clone()).or_insert(*span);
            }
        }
        Expr::Array { elements, .. } => {
            for element in elements {
                collect_implicit_expr(
                    element,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
            }
        }
        Expr::Dict { entries, .. } => {
            for entry in entries {
                collect_implicit_expr(
                    &entry.key,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
                collect_implicit_expr(
                    &entry.value,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
            }
        }
        Expr::Comprehension {
            element,
            iterable,
            condition,
            ..
        } => {
            collect_implicit_expr(
                element,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            collect_implicit_expr(
                iterable,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            if let Some(condition) = condition {
                collect_implicit_expr(
                    condition,
                    declared_globals,
                    declared_players,
                    globals,
                    players,
                );
            }
        }
        Expr::Lambda { body, .. } => {
            collect_implicit_expr(body, declared_globals, declared_players, globals, players)
        }
        Expr::Type { args, .. } => {
            for arg in args {
                collect_implicit_expr(arg, declared_globals, declared_players, globals, players);
            }
        }
        Expr::Vector { x, y, z, .. } => {
            collect_implicit_expr(x, declared_globals, declared_players, globals, players);
            collect_implicit_expr(y, declared_globals, declared_players, globals, players);
            collect_implicit_expr(z, declared_globals, declared_players, globals, players);
        }
        Expr::PlayerVar {
            player,
            name,
            member_span,
            span,
        } => {
            if !declared_players.contains(name.as_str()) && default_var_index(name).is_some() {
                players.entry(name.clone()).or_insert(member_span.or(*span));
            }
            collect_implicit_expr(player, declared_globals, declared_players, globals, players);
        }
        Expr::Member {
            receiver,
            member,
            span,
            ..
        } => {
            if !declared_players.contains(member.as_str()) && default_var_index(member).is_some() {
                players.entry(member.clone()).or_insert(*span);
            }
            collect_implicit_expr(
                receiver,
                declared_globals,
                declared_players,
                globals,
                players,
            );
        }
        Expr::Call { args, .. } | Expr::MacroCall { args, .. } => {
            for arg in args {
                collect_implicit_expr(arg, declared_globals, declared_players, globals, players);
            }
        }
        Expr::ReceiverCall { receiver, args, .. } => {
            collect_implicit_expr(
                receiver,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            for arg in args {
                collect_implicit_expr(arg, declared_globals, declared_players, globals, players);
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_implicit_expr(left, declared_globals, declared_players, globals, players);
            collect_implicit_expr(right, declared_globals, declared_players, globals, players);
        }
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => {
            collect_implicit_expr(
                then_value,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            collect_implicit_expr(
                condition,
                declared_globals,
                declared_players,
                globals,
                players,
            );
            collect_implicit_expr(
                else_value,
                declared_globals,
                declared_players,
                globals,
                players,
            );
        }
        Expr::Unary { operand, .. } => collect_implicit_expr(
            operand,
            declared_globals,
            declared_players,
            globals,
            players,
        ),
        Expr::Index { array, index, .. } => {
            collect_implicit_expr(array, declared_globals, declared_players, globals, players);
            collect_implicit_expr(index, declared_globals, declared_players, globals, players);
        }
        Expr::Format { args, .. } => {
            for arg in args {
                collect_implicit_expr(arg, declared_globals, declared_players, globals, players);
            }
        }
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Null { .. }
        | Expr::StringModifier { .. }
        | Expr::Local { .. }
        | Expr::Enum { .. }
        | Expr::EventPlayer { .. }
        | Expr::HostPlayer { .. }
        | Expr::Constant { .. }
        | Expr::MacroParam { .. } => {}
    }
}

fn allocate_indices(
    entries: &[(Option<u32>, Option<HirSpan>)],
    pre_reserved: &HashSet<u32>,
    kind: &str,
) -> Result<Vec<u32>, IntegrationError> {
    let mut reserved = pre_reserved.clone();
    for (index, span) in entries {
        let Some(index) = index else {
            continue;
        };
        if !reserved.insert(*index) {
            return Err(IntegrationError::new(
                "index-collision",
                format!("duplicate explicit {kind} index {index}"),
                *span,
            ));
        }
    }

    // The pinned OverPy reference fills the remaining free slots in
    // ascending order for auto-allocated entries, regardless of where the
    // explicit indices sit in declaration order; an early explicit index
    // does not push later auto allocations above it.
    let mut next = 0;
    let mut allocated = Vec::with_capacity(entries.len());
    for (index, span) in entries {
        let assigned = if let Some(index) = index {
            *index
        } else {
            while reserved.contains(&next) {
                next = next.checked_add(1).ok_or_else(|| {
                    IntegrationError::new(
                        "index-exhausted",
                        format!("no available {kind} index remains"),
                        *span,
                    )
                })?;
            }
            reserved.insert(next);
            let assigned = next;
            next = next.checked_add(1).ok_or_else(|| {
                IntegrationError::new(
                    "index-exhausted",
                    format!("no available {kind} index remains"),
                    *span,
                )
            })?;
            assigned
        };
        allocated.push(assigned);
    }
    Ok(allocated)
}

fn player_event_kind(name: &str) -> Option<PlayerEventKind> {
    Some(match name {
        "playerDealtDamage" => PlayerEventKind::DealtDamage,
        "playerDealtFinalBlow" => PlayerEventKind::DealtFinalBlow,
        "playerDealtHealing" => PlayerEventKind::DealtHealing,
        "playerDied" => PlayerEventKind::Died,
        "playerEarnedElimination" => PlayerEventKind::EarnedElimination,
        "playerJoined" => PlayerEventKind::Joined,
        "playerLeft" => PlayerEventKind::Left,
        "playerReceivedHealing" => PlayerEventKind::ReceivedHealing,
        "playerTookDamage" => PlayerEventKind::TookDamage,
        _ => return None,
    })
}

fn is_zero_initializer(expr: &hir::Expr) -> bool {
    match expr {
        hir::Expr::Number { text, value, .. } => text == "0" && *value == 0.0,
        hir::Expr::Null { .. } => true,
        _ => false,
    }
}

fn literal_key_matches(left: &hir::Expr, right: &hir::Expr) -> bool {
    match (left, right) {
        (hir::Expr::Number { value: left, .. }, hir::Expr::Number { value: right, .. }) => {
            left == right
        }
        (hir::Expr::String { value: left, .. }, hir::Expr::String { value: right, .. }) => {
            left == right
        }
        (hir::Expr::Bool { value: left, .. }, hir::Expr::Bool { value: right, .. }) => {
            left == right
        }
        (hir::Expr::Null { .. }, hir::Expr::Null { .. }) => true,
        _ => false,
    }
}

fn indexed_target_parts<'a>(
    target: &'a hir::Expr,
    indices: &mut Vec<&'a hir::Expr>,
) -> Option<&'a hir::Expr> {
    match target {
        hir::Expr::Index { array, index, .. } => {
            indices.push(index);
            indexed_target_parts(array, indices)
        }
        hir::Expr::GlobalVar { .. } | hir::Expr::PlayerVar { .. } => Some(target),
        _ => None,
    }
}

fn is_literal_key(expr: &hir::Expr) -> bool {
    matches!(
        expr,
        hir::Expr::Number { .. }
            | hir::Expr::String { .. }
            | hir::Expr::Bool { .. }
            | hir::Expr::Null { .. }
    )
}

fn translation_locale(language: &str) -> Option<&'static str> {
    Some(match language {
        "de" => "de-DE",
        "en" => "en-US",
        "es" | "es_mx" => "es-MX",
        "es_es" => "es-ES",
        "fr" => "fr-FR",
        "it" => "it-IT",
        "ja" => "ja-JP",
        "ko" => "ko-KR",
        "pl" => "pl-PL",
        "pt" => "pt-BR",
        "ru" => "ru-RU",
        "th" => "th-TH",
        "tr" => "tr-TR",
        "zh" | "zh_cn" => "zh-CN",
        "zh_tw" => "zh-TW",
        _ => return None,
    })
}

fn big_letters(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut converted = false;
    for character in value.chars() {
        if !converted {
            if let Some(mapped) = big_letter(character) {
                output.push(mapped);
                converted = true;
                continue;
            }
        }
        output.push(character);
    }
    output
}

fn big_letter(character: char) -> Option<char> {
    Some(match character {
        'a' | 'A' => 'Α',
        'b' | 'B' => 'Β',
        'e' | 'E' => 'Ε',
        'h' | 'H' => 'Η',
        'i' | 'I' => 'Ι',
        'k' | 'K' => 'Κ',
        'm' | 'M' => 'Μ',
        'n' | 'N' => 'Ν',
        'o' | 'O' => 'Ο',
        'p' | 'P' => 'Ρ',
        't' | 'T' => 'Τ',
        'x' | 'X' => 'Χ',
        'y' | 'Y' => 'Υ',
        'z' | 'Z' => 'Ζ',
        '.' => '\u{2024}',
        ' ' => '\u{2028}',
        _ => return None,
    })
}

fn fullwidth(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            ' ' => '\u{2001}',
            '\u{00a5}' => '\u{ffe5}',
            '\u{20a9}' => '\u{ffe6}',
            '\u{00a2}' => '\u{ffe0}',
            '\u{00a3}' => '\u{ffe1}',
            '\u{00af}' => '\u{ffe3}',
            '\u{00ac}' => '\u{ffe2}',
            '\u{00a6}' => '\u{ffe4}',
            character if ('!'..='~').contains(&character) => {
                char::from_u32(character as u32 + 65248).unwrap_or(character)
            }
            _ => character,
        })
        .collect()
}

fn case_sensitive(value: &str) -> String {
    let mut output = value.replace('æ', "\u{04d5}").replace("nj", "\u{01cc}");
    output = output.replace(" a ", " ａ ");
    output
        .chars()
        .map(|character| match character {
            'a' => 'ạ',
            'b' => 'ḅ',
            'c' => 'ƈ',
            'd' => 'ḍ',
            'e' => 'ẹ',
            'f' => 'ƒ',
            'g' => 'ǥ',
            'h' => '\u{04bb}',
            'i' => 'і',
            'j' => 'ј',
            'k' => 'ḳ',
            'l' => 'I',
            'm' => 'ṃ',
            'n' => 'ṇ',
            'o' => 'ο',
            'p' => 'ṗ',
            'q' => 'ǫ',
            'r' => 'ṛ',
            's' => 'ѕ',
            't' => 'ṭ',
            'u' => 'υ',
            'v' => 'ν',
            'w' => 'ẉ',
            'x' => '\u{04b3}',
            'y' => 'ỵ',
            'z' => 'ẓ',
            _ => character,
        })
        .collect()
}

fn canonical_number_text(value: f64, text: &str) -> String {
    if text.starts_with("0x") || text.starts_with("0X") {
        value.to_string()
    } else {
        text.to_string()
    }
}

fn computed_number_text(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn canonical_format_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut index = 0;
    while let Some(character) = chars.next() {
        if character == '{' && chars.peek() == Some(&'}') {
            chars.next();
            output.push('{');
            output.push_str(&index.to_string());
            output.push('}');
            index += 1;
        } else {
            output.push(character);
        }
    }
    output
}

fn fold_literal_format(text: &str, args: &[hir::Expr]) -> Option<String> {
    let values = args
        .iter()
        .map(|arg| match arg {
            hir::Expr::Number { text, value, .. } => Some(canonical_number_text(*value, text)),
            hir::Expr::String { value, .. } => Some(value.clone()),
            hir::Expr::Bool { value, .. } => Some(value.to_string()),
            hir::Expr::Null { .. } => Some("null".to_string()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let mut output = canonical_format_text(text);
    for (index, value) in values.iter().enumerate() {
        output = output.replace(&format!("{{{index}}}"), value);
    }
    Some(output)
}

fn debug_expr_text(expr: &Expr) -> String {
    match expr {
        Expr::Number { text, .. } => text.clone(),
        Expr::String { value, .. } => {
            format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
        }
        Expr::Bool { value, .. } => value.to_string(),
        Expr::Null { .. } => "null".to_string(),
        Expr::Array { elements, .. } => format!(
            "[{}]",
            elements
                .iter()
                .map(debug_expr_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Dict { entries, .. } => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|entry| format!(
                    "{}: {}",
                    debug_expr_text(&entry.key),
                    debug_expr_text(&entry.value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Comprehension {
            element,
            variable,
            iterable,
            condition,
            ..
        } => {
            let condition = condition
                .as_deref()
                .map(|condition| format!(" if {}", debug_expr_text(condition)))
                .unwrap_or_default();
            format!(
                "[{} for {} in {}{}]",
                debug_expr_text(element),
                variable,
                debug_expr_text(iterable),
                condition
            )
        }
        Expr::Lambda { params, body, .. } => {
            format!("lambda {}: {}", params.join(", "), debug_expr_text(body))
        }
        Expr::StringModifier {
            modifier, value, ..
        } => format!("{}\"{}\"", modifier, value),
        Expr::Local { name, .. }
        | Expr::GlobalVar { name, .. }
        | Expr::Constant { name, .. }
        | Expr::MacroParam { name, .. } => name.clone(),
        Expr::Type { name, args, .. } => {
            if args.is_empty() {
                name.clone()
            } else {
                format!(
                    "{}[{}]",
                    name,
                    args.iter()
                        .map(debug_expr_text)
                        .collect::<Vec<_>>()
                        .join(": ")
                )
            }
        }
        Expr::Vector { x, y, z, .. } => format!(
            "vect({}, {}, {})",
            debug_expr_text(x),
            debug_expr_text(y),
            debug_expr_text(z)
        ),
        Expr::Enum {
            value_type, value, ..
        } => format!("{}.{}", value_type, value),
        Expr::PlayerVar { player, name, .. } => {
            format!("{}.{}", debug_expr_text(player), name)
        }
        Expr::Member {
            receiver, member, ..
        } => format!("{}.{}", debug_expr_text(receiver), member),
        Expr::EventPlayer { .. } => "eventPlayer".to_string(),
        Expr::HostPlayer { .. } => "hostPlayer".to_string(),
        Expr::Call { name, args, .. } if name == "sorted" && args.len() == 2 => {
            format!(
                "sorted({}, key = {})",
                debug_expr_text(&args[0]),
                debug_expr_text(&args[1])
            )
        }
        Expr::Call { name, args, .. } | Expr::MacroCall { name, args, .. } => format!(
            "{}({})",
            name,
            args.iter()
                .map(debug_expr_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } => format!(
            "{}.{}({})",
            debug_expr_text(receiver),
            name,
            args.iter()
                .map(debug_expr_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Binary {
            left, op, right, ..
        } => format!(
            "{} {} {}",
            debug_expr_text(left),
            op,
            debug_expr_text(right)
        ),
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => format!(
            "{} if {} else {}",
            debug_expr_text(then_value),
            debug_expr_text(condition),
            debug_expr_text(else_value)
        ),
        Expr::Unary { op, operand, .. } => format!("{} {}", op, debug_expr_text(operand)),
        Expr::Index { array, index, .. } => {
            format!("{}[{}]", debug_expr_text(array), debug_expr_text(index))
        }
        Expr::Format { text, args, .. } => format!(
            "\"{}\".format({})",
            text,
            args.iter()
                .map(debug_expr_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn canonical_debug_text(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            'a' => 'ạ',
            'b' => 'ḅ',
            'c' => 'ƈ',
            'd' => 'ḍ',
            'e' => 'ẹ',
            'f' => 'ƒ',
            'g' => 'ǥ',
            'h' => 'һ',
            'i' => 'і',
            'j' => 'ј',
            'k' => 'ḳ',
            'l' => 'I',
            'm' => 'ṃ',
            'n' => 'ṇ',
            'o' => 'ο',
            'p' => 'ṗ',
            'q' => 'ǫ',
            'r' => 'ṛ',
            's' => 'ѕ',
            't' => 'ṭ',
            'u' => 'υ',
            'v' => 'ν',
            'w' => 'ẉ',
            'x' => 'ҳ',
            'y' => 'ỵ',
            'z' => 'ẓ',
            _ => character,
        })
        .collect()
}

fn negated_comparison(op: &str) -> Option<&'static str> {
    Some(match op {
        "==" => "!=",
        "!=" => "==",
        "<" => ">=",
        ">" => "<=",
        "<=" => ">",
        ">=" => "<",
        _ => return None,
    })
}

fn modify_op_from_str(op: &str) -> Option<wir::ModifyOp> {
    match op {
        "+" => Some(wir::ModifyOp::Add),
        "-" => Some(wir::ModifyOp::Subtract),
        "*" => Some(wir::ModifyOp::Multiply),
        "/" => Some(wir::ModifyOp::Divide),
        "%" => Some(wir::ModifyOp::Modulo),
        "**" => Some(wir::ModifyOp::RaiseToPower),
        _ => None,
    }
}

fn modify_catalog_name_from_str(op: &str) -> Option<&'static str> {
    match op {
        "+" => Some("add"),
        "-" => Some("subtract"),
        "*" => Some("multiply"),
        "/" => Some("divide"),
        "%" => Some("modulo"),
        "**" => Some("raiseToPower"),
        _ => None,
    }
}

fn hir_span_from_diag(span: crate::diag::Span) -> HirSpan {
    HirSpan {
        file: span.file,
        start: hir::Position {
            line: span.start.line,
            col: span.start.col,
        },
        end: hir::Position {
            line: span.end.line,
            col: span.end.col,
        },
    }
}

fn workshop_error_span(error: &workshop_rs::WorkshopError) -> Option<WorkshopSpan> {
    match error {
        workshop_rs::WorkshopError::Unknown { span, .. }
        | workshop_rs::WorkshopError::Malformed { span, .. }
        | workshop_rs::WorkshopError::Unsupported { span, .. } => *span,
        workshop_rs::WorkshopError::Catalog(_)
        | workshop_rs::WorkshopError::MissingMapping { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMPILE_SCHEMA_VERSION, CompileFailureClass, CompileStatus, Compiler, WORKSHOP_RS_VERSION,
        cross_check_manifest,
    };
    use crate::manifest::Manifest;
    use std::path::Path;
    use workshop_rs::catalog::{Catalog, Locale};

    #[test]
    fn public_contract_is_pinned_and_manifest_links_are_checked() {
        let compiler = Compiler::new().expect("released workshop contract must load");
        let identity = compiler.catalog_identity();
        assert_eq!(identity.implementation_version, WORKSHOP_RS_VERSION);
        assert!(compiler.link_report().catalog_ids_checked > 0);
        assert!(compiler.link_report().domains_checked > 0);
    }

    #[test]
    fn compile_report_is_versioned_and_contains_reproducibility_identity() {
        let compiler = Compiler::new().unwrap();
        let report = compiler.compile_source_report_with_locale(
            "rule \"report\":\n    @Event global\n    disableInspector()\n",
            "report.opy",
            Path::new("."),
            &Locale::new("en-US"),
        );
        assert_eq!(report.schema_version, COMPILE_SCHEMA_VERSION);
        assert_eq!(report.compiler.name, "opy-rs");
        assert_eq!(report.catalog.implementation_version, WORKSHOP_RS_VERSION);
        assert_eq!(report.compile.status, CompileStatus::Success);
        assert_eq!(report.compile.exit_code, 0);
        assert!(report.compile.diagnostics.is_empty());
        assert_eq!(
            report.compile.workshop,
            report
                .compile
                .workshop_exact
                .trim_end_matches('\n')
                .to_owned()
                + "\n"
        );
        assert!(serde_json::to_value(report).unwrap()["catalog"]["catalog-version"].is_string());
    }

    #[test]
    fn compile_report_preserves_frontend_failure_class_and_source_path() {
        let compiler = Compiler::new().unwrap();
        let report = compiler.compile_source_report_with_locale(
            "rule \"broken\":\n    @Event global\n    missing()\n",
            "broken.opy",
            Path::new("."),
            &Locale::new("en-US"),
        );
        assert_eq!(report.compile.status, CompileStatus::Failure);
        assert_eq!(
            report.compile.failure_class,
            Some(CompileFailureClass::Frontend)
        );
        assert_eq!(report.compile.exit_code, 1);
        let diagnostic = &report.compile.diagnostics[0];
        assert_eq!(diagnostic.code, "unknown-action");
        assert_eq!(diagnostic.span.as_ref().unwrap().path, "broken.opy");
    }

    #[test]
    fn compile_report_preserves_integration_failure_class_and_source_path() {
        let compiler = Compiler::new().unwrap();
        let report = compiler.compile_source_report_with_locale(
            "rule \"broken\":\n    @Event global\n    {\"a\": 1}[\"b\"] = 3\n",
            "broken.opy",
            Path::new("."),
            &Locale::new("en-US"),
        );
        assert_eq!(report.compile.status, CompileStatus::Failure);
        assert_eq!(
            report.compile.failure_class,
            Some(CompileFailureClass::Integration)
        );
        assert_eq!(
            report.compile.diagnostics[0].span.as_ref().unwrap().path,
            "broken.opy"
        );
    }

    #[test]
    fn compile_report_preserves_frontend_warnings_on_integration_failure() {
        let compiler = Compiler::new().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compatibility/fixtures/synthetic/preprocessing");
        let report = compiler.compile_source_report_with_locale(
            concat!(
                "#!include \"shared.opy\"\n",
                "#!include \"shared.opy\"\n",
                "rule \"broken\":\n",
                "    @Event global\n",
                "    {\"a\": 1}[\"b\"] = 3\n",
            ),
            "broken.opy",
            &root,
            &Locale::new("en-US"),
        );
        assert_eq!(report.compile.status, CompileStatus::Failure);
        assert_eq!(
            report.compile.failure_class,
            Some(CompileFailureClass::Integration)
        );
        assert_eq!(report.compile.diagnostics.len(), 2);
        assert_eq!(
            report.compile.diagnostics[0].severity,
            crate::tooling::DiagnosticSeverity::Warning
        );
        assert_eq!(report.compile.diagnostics[0].code, "w_already_imported");
        assert_eq!(
            report.compile.diagnostics[1].severity,
            crate::tooling::DiagnosticSeverity::Error
        );
        assert_eq!(
            report.compile.diagnostics[1].span.as_ref().unwrap().path,
            "broken.opy"
        );
    }

    #[test]
    fn vertical_slice_preserves_source_files_spans_and_emits_workshop() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar A\nrule \"issue 35 integration\":\n    @Event global\n    A = 1\n    disableInspector()\n",
            "issue-35-integration.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert_eq!(
            artifact
                .wir
                .files
                .get(workshop_rs::source::FileId::from_index(0))
                .unwrap()
                .path,
            "issue-35-integration.opy"
        );
        let rule = artifact
            .wir
            .rules
            .get(workshop_rs::wir::RuleId::from_index(0))
            .unwrap();
        assert_eq!(rule.span.unwrap().file.index(), 0);
        assert_eq!(rule.name_span.unwrap().start.line, 2);
        assert!(artifact.emitted.contains("Disable Inspector Recording;"));
        assert_eq!(artifact.catalog_identity.implementation_version, "0.1.16");
    }

    #[test]
    fn stale_catalog_links_fail_explicitly() {
        let manifest = Manifest::builtin().unwrap().clone();
        let mut stale = manifest;
        stale.functions[0].catalog_id = Some("missing-catalog-id".to_string());
        let error = cross_check_manifest(&stale, &Catalog::builtin().unwrap()).unwrap_err();
        assert_eq!(error.diagnostic.code, "catalog-link-missing");
    }

    #[test]
    fn while_lowering_is_source_attributed() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "rule \"while\":\n    @Event global\n    while true:\n        disableInspector()\n",
            "while.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let rule = artifact
            .wir
            .rules
            .get(workshop_rs::wir::RuleId::from_index(0))
            .unwrap();
        assert!(matches!(
            artifact.wir.actions.get(rule.actions[0]),
            Some(workshop_rs::wir::Action::While { .. })
        ));
        assert!(artifact.emitted.contains("While(True);"));
    }

    #[test]
    fn structural_subroutines_lower_to_canonical_wir() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar score\nsubroutine showStatus\ndef showStatus():\n    @Name \"Friendly\"\n    @SuppressWarnings unusedVariable\n    disableInspector()\nrule \"caller\":\n    @Event global\n    showStatus()\n",
            "structure.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let subroutine = artifact
            .wir
            .subroutines
            .get(workshop_rs::wir::SubroutineId::from_index(0))
            .unwrap();
        assert_eq!(subroutine.name, "showStatus");
        assert_eq!(subroutine.index, 0);
        assert_eq!(subroutine.name_span.unwrap().start.line, 2);
        assert_eq!(artifact.wir.rules.len(), 2);
        let subroutine_rule = artifact
            .wir
            .rules
            .get(workshop_rs::wir::RuleId::from_index(0))
            .unwrap();
        let workshop_rs::wir::Event::Subroutine(subroutine_id) = subroutine_rule.event else {
            panic!("expected a subroutine event");
        };
        assert_eq!(
            artifact.wir.subroutines.get(subroutine_id).unwrap().name,
            "showStatus"
        );
        assert!(matches!(
            artifact
                .wir
                .actions
                .get(workshop_rs::wir::ActionId::from_index(1))
                .unwrap(),
            workshop_rs::wir::Action::CallSubroutine { .. }
        ));
        assert!(artifact.emitted.contains("Subroutine Friendly"));
    }

    #[test]
    fn player_event_filters_resolve_through_canonical_catalog() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "rule \"joined\":\n    @Event playerJoined\n    @Team 1\n    @Slot 2\n    disableInspector()\n",
            "filters.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(matches!(
            &artifact
                .wir
                .rules
                .get(workshop_rs::wir::RuleId::from_index(0))
                .unwrap()
                .event,
            workshop_rs::wir::Event::Player {
                kind: workshop_rs::wir::PlayerEventKind::Joined,
                team: workshop_rs::wir::EventTeam::Team1,
                target: workshop_rs::wir::EventTarget::Slot(2),
            }
        ));
        assert!(artifact.emitted.contains("Player Joined Match;"));
    }

    #[test]
    fn explicit_indices_are_reserved_before_deterministic_allocation() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar first\nglobalvar reserved 0\nglobalvar next\nrule \"indices\":\n    @Event global\n    disableInspector()\n",
            "indices.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let by_name = artifact
            .wir
            .global_variables
            .iter()
            .map(|variable| (variable.name.as_str(), variable.index))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            by_name,
            std::collections::BTreeMap::from([("first", 1), ("reserved", 0), ("next", 2)])
        );
        // Variable tables are emitted in Workshop index order.
        let indices = artifact
            .wir
            .global_variables
            .iter()
            .map(|variable| variable.index)
            .collect::<Vec<_>>();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn implicit_default_variables_use_reference_fixed_slots() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            r#"
globalvar timer
globalvar extra 5

rule "implicit":
    @Event global
    A = timer + 1
    B = A
    B += 2
    A[0] = 7
    DX = B * A
"#,
            "implicit.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let globals = artifact
            .wir
            .global_variables
            .iter()
            .map(|variable| (variable.name.clone(), variable.index))
            .collect::<Vec<_>>();
        // The implicit A (0), B (1), and DX (127) names keep their fixed
        // Workshop slots and reserve them for declared-variable allocation
        // (pinned OverPy evidence); `timer` auto-allocates around them and
        // `extra` keeps its explicit index.
        assert_eq!(
            globals,
            vec![
                ("A".to_string(), 0),
                ("B".to_string(), 1),
                ("timer".to_string(), 2),
                ("extra".to_string(), 5),
                ("DX".to_string(), 127),
            ]
        );
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable(A, Add(Global.timer, 1));")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable(B, Global.A);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable(B, Add, 2);")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable At Index(A, 0, 7);")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable(DX, Multiply(Global.B, Global.A));")
        );
    }

    #[test]
    fn implicit_default_variable_slot_collision_is_source_attributed() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar x 0\nrule \"collision\":\n    @Event global\n    x = 1\n    A = 2\n",
            "collision.opy",
            Path::new("."),
        )
        .unwrap();
        let error = match compiler.compile_hir(&hir) {
            Ok(_) => panic!("slot collision unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "index-collision");
        assert_eq!(error.diagnostic.span.unwrap().start.line, 5);
        assert!(error.diagnostic.message.contains("'A' and 'x'"));
    }

    #[test]
    fn implicit_default_player_variables_use_independent_reference_slots() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            r#"
playervar declaredPlayer

rule "implicit player variables":
    @Event eachPlayer
    A = 1
    eventPlayer.A = 1
    eventPlayer.A += 2
    eventPlayer.E = eventPlayer.A
    eventPlayer.DX = eventPlayer.E
    eventPlayer.declaredPlayer = eventPlayer.A
"#,
            "implicit-player.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let globals = artifact
            .wir
            .global_variables
            .iter()
            .map(|variable| (variable.name.as_str(), variable.index))
            .collect::<std::collections::BTreeMap<_, _>>();
        let players = artifact
            .wir
            .player_variables
            .iter()
            .map(|variable| (variable.name.as_str(), variable.index))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(globals.get("A"), Some(&0));
        assert_eq!(players.get("A"), Some(&0));
        assert_eq!(players.get("declaredPlayer"), Some(&1));
        assert_eq!(players.get("E"), Some(&4));
        assert_eq!(players.get("DX"), Some(&127));
        assert!(
            artifact
                .emitted
                .contains("Set Player Variable(Event Player, A, 1);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Player Variable(Event Player, A, Add, 2);")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Player Variable(Event Player, E, (Event Player).A);")
        );
    }

    #[test]
    fn implicit_default_player_slot_collision_is_source_attributed() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "playervar declared 0\nrule \"collision\":\n    @Event eachPlayer\n    eventPlayer.A = 1\n",
            "player-collision.opy",
            Path::new("."),
        )
        .unwrap();
        let error = match compiler.compile_hir(&hir) {
            Ok(_) => panic!("player slot collision unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "index-collision");
        assert!(
            error
                .diagnostic
                .message
                .contains("player variables 'A' and 'declared'")
        );
        assert_eq!(error.diagnostic.span.unwrap().start.line, 4);
    }

    #[test]
    fn power_augmented_assignment_lowers_from_source() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar g\nrule \"power\":\n    @Event global\n    g = 2\n    g **= 3\n",
            "power.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(artifact.emitted.contains("Set Global Variable(g, 2);"));
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable(g, Raise To Power, 3);")
        );
    }

    #[test]
    fn opy_hex_numbers_are_normalized_at_the_wir_boundary() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar large = 0x124BC\nglobalvar small = 0x124\nglobalvar scientific = 1e10\n",
            "numbers.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable(large, 74940);")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable(small, 292);")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable(scientific, 1e10);")
        );
        assert!(!artifact.emitted.contains("0x124BC"));
        assert!(!artifact.emitted.contains("0x124"));
    }

    #[test]
    fn literal_dict_lookup_lowers_to_the_selected_value() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar total\nrule \"negative\":\n    @Event global\n    total = {\"a\": 1, \"b\": 2}[\"a\"]\n",
            "negative.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler
            .compile_hir(&hir)
            .expect("literal dict lookup should lower");
        assert!(artifact.emitted.contains("Set Global Variable(total, 1);"));
    }

    #[test]
    fn auto_allocation_fills_free_slots_below_early_explicit_indices() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            r#"
globalvar reserved 5
globalvar auto1
globalvar auto2

rule "allocation":
    @Event global
    auto1 = 1
    auto2 = 2
    B = 3
"#,
            "allocation.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let by_name = artifact
            .wir
            .global_variables
            .iter()
            .map(|variable| (variable.name.clone(), variable.index))
            .collect::<std::collections::BTreeMap<_, _>>();
        // The implicit B keeps its fixed slot 1; the auto-allocated variables
        // fill the remaining free slots below the explicit 5 instead of
        // jumping past it, matching the pinned OverPy oracle (slot 0 stays
        // free here because the implicit A is never used).
        assert_eq!(
            by_name,
            std::collections::BTreeMap::from([
                ("B".to_string(), 1),
                ("auto1".to_string(), 0),
                ("auto2".to_string(), 2),
                ("reserved".to_string(), 5),
            ])
        );
    }

    #[test]
    fn power_expressions_lower_through_the_canonical_contract() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar a = [2, 4]\nglobalvar out\nrule \"power\":\n    @Event global\n    out = a ** 2\n    a **= 2\n    a[0] **= 2\n",
            "power.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable(out, Raise To Power(Global.a, 2));")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable(a, Raise To Power, 2);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable At Index(a, 0, Raise To Power, 2);")
        );
    }

    #[test]
    fn unsupported_rule_metadata_is_explicit_and_source_attributed() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "rule \"metadata\":\n    @Event global\n    @NewPage \"section\"\n    disableInspector()\n",
            "metadata.opy",
            Path::new("."),
        )
        .unwrap();
        let error = match compiler.compile_hir(&hir) {
            Ok(_) => panic!("unsupported metadata unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
        assert_eq!(error.diagnostic.span.unwrap().start.line, 3);
    }

    #[test]
    fn issue_40_oracle_fixture_and_wir_lowering_agree() {
        let compiler = Compiler::new().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compatibility/fixtures/synthetic/issue-40-structural");
        let source = std::fs::read_to_string(fixture.join("source.opy")).unwrap();
        let hir = crate::compile(&source, "source.opy", &fixture).unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture.join("oracle.json")).unwrap())
                .unwrap();
        let oracle_workshop = oracle["compile"]["workshop"].as_str().unwrap();
        let oracle_wir = workshop_rs::parser::parse(
            oracle_workshop,
            &Catalog::builtin().unwrap(),
            &Locale::new("en-US"),
        )
        .unwrap();
        assert!(workshop_rs::roundtrip::equivalent(
            &artifact.wir,
            &oracle_wir
        ));

        assert!(oracle_workshop.contains("0: reserved"));
        assert!(oracle_workshop.contains("1: first"));
        assert!(oracle_workshop.contains("2: explicit"));
        assert!(oracle_workshop.contains("3: next"));
        assert!(oracle_workshop.contains("0: helper"));
        assert!(oracle_workshop.contains("Subroutine;\n        helper;"));
        assert!(oracle_workshop.contains("Player Joined Match;\n        Team 1;\n        Slot 2;"));

        let indices = artifact
            .wir
            .global_variables
            .iter()
            .map(|variable| variable.index)
            .collect::<Vec<_>>();
        assert_eq!(indices, vec![0, 1, 2, 3]);
        assert_eq!(
            artifact.wir.subroutines.iter().next().unwrap().name,
            "helper"
        );
        assert!(artifact.emitted.contains("[Source] renamed helper"));
        assert!(matches!(
            artifact
                .wir
                .rules
                .get(workshop_rs::wir::RuleId::from_index(1))
                .unwrap()
                .event,
            workshop_rs::wir::Event::Player {
                kind: workshop_rs::wir::PlayerEventKind::Joined,
                team: workshop_rs::wir::EventTeam::Team1,
                target: workshop_rs::wir::EventTarget::Slot(2),
            }
        ));
    }

    #[test]
    fn assignments_and_modifications_lower_to_canonical_wir() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            r#"
globalvar g1
globalvar g2
playervar p1
playervar p2 = [1, 2, 3]

rule "assignments":
    @Event eachPlayer
    g1 = 10
    g1 += 5
    g1 -= 2
    g1 *= 3
    g1 /= 2
    g1 %= 4
    g2 = [1, 2, 3]
    g2[0] = 99
    g2[1] += 1
    eventPlayer.p1 = 42
    eventPlayer.p1 += 8
    eventPlayer.p1 *= 2
    eventPlayer.p2[2] = 7
    eventPlayer.p2[0] -= 3
"#,
            "assign.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(artifact.emitted.contains("Set Global Variable(g1, 10);"));
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable(g1, Add, 5);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable(g1, Subtract, 2);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable(g1, Multiply, 3);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable(g1, Divide, 2);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable(g1, Modulo, 4);")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Global Variable At Index(g2, 0, 99);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Global Variable At Index(g2, 1, Add, 1);")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Player Variable(Event Player, p1, 42);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Player Variable(Event Player, p1, Add, 8);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Player Variable(Event Player, p1, Multiply, 2);")
        );
        assert!(
            artifact
                .emitted
                .contains("Set Player Variable At Index((Event Player).p2, 2, 7);")
        );
        assert!(
            artifact
                .emitted
                .contains("Modify Player Variable At Index((Event Player).p2, 0, Subtract, 3);")
        );

        // Direct assignments carry both the statement span and the separate
        // target-variable span; indexed forms lower to Call actions that
        // carry only the statement span.
        let rule = artifact
            .wir
            .rules
            .get(workshop_rs::wir::RuleId::from_index(1))
            .unwrap();
        let direct = artifact.wir.actions.get(rule.actions[0]).unwrap();
        match direct {
            workshop_rs::wir::Action::SetGlobalVariable {
                span,
                target_span,
                variable,
                ..
            } => {
                assert_eq!(span.unwrap().start.line, 9);
                assert_eq!(target_span.unwrap().start.line, 9);
                assert_eq!(
                    artifact.wir.global_variables.get(*variable).unwrap().name,
                    "g1"
                );
            }
            other => panic!("expected a direct global assignment, got {other:?}"),
        }
        let indexed = artifact.wir.actions.get(rule.actions[7]).unwrap();
        match indexed {
            workshop_rs::wir::Action::Call { span, .. } => {
                assert_eq!(span.unwrap().start.line, 16);
            }
            other => panic!("expected an indexed assignment call, got {other:?}"),
        }
    }

    #[test]
    fn expressions_and_values_lower_to_canonical_wir() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            r#"
enum Consts:
    BASE

globalvar total
globalvar arr = [1, 2, 3]
globalvar pos = vect(1, 2, 3)

rule "expressions":
    @Event global
    @Condition total == 0
    @Condition not (pos == vect(0, 0, 0))
    @Condition 2 in arr
    total = Consts.BASE + arr[1] * 2 - (10 / 2) + (5 % 2)
    print("Total: {}".format(total))
    debug(pos)
"#,
            "expr.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(artifact.emitted.contains("Global.total == 0;"));
        // `not (pos == vect(0, 0, 0))` lowers to the negated comparison,
        // mirroring the pinned OverPy oracle.
        assert!(artifact.emitted.contains("Global.pos != Vector(0, 0, 0);"));
        assert!(
            artifact
                .emitted
                .contains("Array Contains(Global.arr, 2) == True;")
        );
        assert!(
            artifact
                .emitted
                .contains("Custom String(\"Total: {0}\", Global.total)")
        );
    }

    #[test]
    fn pass_is_supported_as_source_level_noop() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            r#"
subroutine emptySub

def emptySub():
    pass

rule "empty rule":
    @Event global
    pass
"#,
            "pass.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let rule0 = artifact
            .wir
            .rules
            .get(workshop_rs::wir::RuleId::from_index(0))
            .unwrap();
        assert!(rule0.actions.is_empty());
        let rule1 = artifact
            .wir
            .rules
            .get(workshop_rs::wir::RuleId::from_index(1))
            .unwrap();
        assert!(rule1.actions.is_empty());
    }

    #[test]
    fn variable_initializers_synthesize_initialize_rules() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            r#"
globalvar j = 5
globalvar h = 0
globalvar k = 0.0
playervar p = 7
playervar q = 0

rule "main":
    @Event global
    disableInspector()
"#,
            "init.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert_eq!(
            artifact
                .wir
                .rules
                .get(workshop_rs::wir::RuleId::from_index(0))
                .unwrap()
                .name,
            "Initialize global variables"
        );
        assert_eq!(
            artifact
                .wir
                .rules
                .get(workshop_rs::wir::RuleId::from_index(1))
                .unwrap()
                .name,
            "Initialize player variables"
        );
        assert_eq!(
            artifact
                .wir
                .rules
                .get(workshop_rs::wir::RuleId::from_index(2))
                .unwrap()
                .name,
            "main"
        );
        assert!(artifact.emitted.contains("Set Global Variable(j, 5);"));
        assert!(artifact.emitted.contains("Set Global Variable(k, 0.0);"));
        assert!(!artifact.emitted.contains("Set Global Variable(h,"));
        assert!(
            artifact
                .emitted
                .contains("Set Player Variable(Event Player, p, 7);")
        );
        assert!(
            !artifact
                .emitted
                .contains("Set Player Variable(Event Player, q,")
        );
    }

    #[test]
    fn settings_lower_through_workshop_owned_emission() {
        let compiler = Compiler::new().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compatibility/fixtures/synthetic/settings");
        let source = std::fs::read_to_string(fixture.join("source.opy")).unwrap();
        let hir = crate::compile(&source, "source.opy", &fixture).unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture.join("oracle.json")).unwrap())
                .unwrap();
        let expected = oracle["compile"]["workshop"]
            .as_str()
            .unwrap()
            .split("\n\nrule")
            .next()
            .unwrap();
        let actual = artifact.emitted.split("\n\nrule").next().unwrap();
        let oracle_wir = workshop_rs::parser::parse(
            oracle["compile"]["workshop"].as_str().unwrap(),
            &Catalog::builtin().unwrap(),
            &Locale::new("en-US"),
        )
        .unwrap();
        assert!(workshop_rs::roundtrip::equivalent(
            &artifact.wir,
            &oracle_wir
        ));
        assert_eq!(
            normalize_workshop_structural_whitespace(actual),
            normalize_workshop_structural_whitespace(expected)
        );
    }

    #[test]
    fn unsupported_locale_has_no_fabricated_source_span() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "#!translations en\nrule \"r\":\n    @Event global\n    pass\n",
            "locale.opy",
            Path::new("."),
        )
        .unwrap();
        let error = match compiler.compile_hir_with_locale(&hir, &Locale::new("xx-XX")) {
            Ok(_) => panic!("unsupported locale unexpectedly compiled"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "locale-unsupported");
        assert_eq!(error.diagnostic.span, None);
    }

    #[test]
    fn locale_selection_emits_catalog_localized_workshop() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "rule \"locale\":\n    @Event global\n    disableInspector()\n",
            "locale.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler
            .compile_hir_with_locale(&hir, &Locale::new("zh-CN"))
            .unwrap();
        assert!(artifact.emitted.contains("规则 (\"locale\")"));
        assert!(artifact.emitted.contains("禁用查看器录制"));
    }

    #[test]
    fn unsupported_backend_directives_fail_at_their_source_anchor() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "#!replace0ByCapturePercentage\nrule \"r\":\n    @Event global\n    pass\n",
            "directives.opy",
            Path::new("."),
        )
        .unwrap();
        let error = match compiler.compile_hir(&hir) {
            Ok(_) => panic!("backend directive unexpectedly compiled"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "backend-directive-unsupported");
        assert_eq!(error.diagnostic.span.unwrap().start.line, 1);
    }

    #[test]
    fn optimizer_directives_remain_non_blocking_presentation_controls() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "#!disableOptimizations\nrule \"r\":\n    @Event global\n    pass\n",
            "optimization.opy",
            Path::new("."),
        )
        .unwrap();
        compiler.compile_hir(&hir).unwrap();
    }

    #[test]
    fn replacement_directive_records_are_checked_even_if_final_state_is_restored() {
        let compiler = Compiler::new().unwrap();
        let mut hir = crate::compile(
            "#!replace0ByCapturePercentage\nrule \"r\":\n    @Event global\n    pass\n",
            "directives.opy",
            Path::new("."),
        )
        .unwrap();
        hir.preprocessing.replacements.clear();
        let error = match compiler.compile_hir(&hir) {
            Ok(_) => panic!("replacement directive unexpectedly compiled"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "backend-directive-unsupported");
        assert_eq!(error.diagnostic.span.unwrap().start.line, 1);
    }

    #[test]
    fn active_replacement_state_is_checked_without_directive_history() {
        let compiler = Compiler::new().unwrap();
        let mut hir = crate::compile(
            "#!replace0ByCapturePercentage\nrule \"r\":\n    @Event global\n    pass\n",
            "directives.opy",
            Path::new("."),
        )
        .unwrap();
        hir.preprocessing.directives.clear();
        hir.preprocessing.replacements[0].span = None;
        let error = match compiler.compile_hir(&hir) {
            Ok(_) => panic!("active replacement state unexpectedly compiled"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "backend-directive-unsupported");
        assert_eq!(error.diagnostic.span, None);
    }

    #[test]
    fn post_compile_hook_receives_exact_emitted_workshop() {
        let compiler = Compiler::new().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macros");
        let source = "#!postCompileHook \"hook.js\"\n\nrule \"setup\":\n    pass\n";
        let artifact = compiler
            .compile_source_with_locale(source, "hook.opy", &root, &Locale::new("en-US"))
            .unwrap();
        assert!(artifact.emitted.contains("rule (\"setup\")"));
        assert!(artifact.final_output.contains("rule (\"transformed\")"));
        assert_ne!(artifact.final_output, artifact.emitted);
    }

    #[test]
    fn post_compile_hook_failure_keeps_script_provenance_and_directive_anchor() {
        let compiler = Compiler::new().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macros");
        let source = "#!postCompileHook \"hook-boom.js\"\n\nrule \"setup\":\n    pass\n";
        let error = match compiler.compile_source_with_locale(
            source,
            "hook.opy",
            &root,
            &Locale::new("en-US"),
        ) {
            Ok(_) => panic!("failing post-compile hook unexpectedly compiled"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "post-compile-hook");
        assert_eq!(error.diagnostic.span.unwrap().start.line, 1);
        let script = error.diagnostic.script.unwrap();
        assert_eq!(script.source_name.as_deref(), Some("hook-boom.js"));
        assert_eq!(script.line, Some(1));
        assert!(script.stack.unwrap().contains("hook-boom.js:1"));
    }

    fn normalize_workshop_structural_whitespace(text: &str) -> String {
        let mut normalized = String::with_capacity(text.len());
        let mut quote = None;
        let mut escaped = false;
        for character in text.chars() {
            if let Some(delimiter) = quote {
                normalized.push(character);
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == delimiter {
                    quote = None;
                }
            } else if matches!(character, '\"' | '\'') {
                quote = Some(character);
                normalized.push(character);
            } else if !character.is_whitespace() {
                normalized.push(character);
            }
        }
        normalized
    }

    #[test]
    fn settings_whitespace_normalization_preserves_quoted_values() {
        assert_ne!(
            normalize_workshop_structural_whitespace("Description: \"a b\""),
            normalize_workshop_structural_whitespace("Description: \"ab\"")
        );
    }
}
