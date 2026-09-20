//! OPY-to-Workshop integration, kept behind the `opy-rs` library boundary.
//!
//! This module consumes the `workshop-rs` 0.3 contract, checks the OPY
//! manifest links against the canonical catalog, and lowers the supported OPY
//! program structure into the canonical Workshop `Program` before validation and deterministic
//! Workshop emission.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::hir::{self, Expr, RuleEntry, Span as HirSpan, Stmt, SwitchArm, default_var_index};
use crate::manifest::{FunctionKind, Manifest};
use serde::Serialize;
use workshop_rs::Program;
use workshop_rs::catalog::{Catalog, CatalogIdentity, Kind, Locale, ParamCoercions};

pub mod reconstruct;

mod backend;
mod blizzard_global;
mod hooks;
mod integration;
mod lowering;
mod settings;

pub(crate) use backend::MacroExpander;
pub(super) use backend::{expand_macros, reject_unlowered_directives};
pub use integration::LinkReport;
pub(super) use integration::load_compiler_contract;
pub(super) use lowering::Lowering;

#[cfg(test)]
mod integration_tests;

const TRANSLATION_HELPER_NAME: &str = "__overpyTranslationHelper__";

fn has_directive(hir: &hir::Program, name: &str) -> bool {
    hir.preprocessing
        .directives
        .iter()
        .any(|directive| directive.name == name)
}

fn omit_declaration_sections(output: &str, program: &workshop_rs::Program) -> String {
    let mut markers = Vec::new();
    for variable in program
        .global_variables
        .iter()
        .chain(program.player_variables.iter())
    {
        markers.push(variable.index.map_or_else(
            || variable.name.clone(),
            |index| format!("{index}: {}", variable.name),
        ));
    }
    markers.extend(program.subroutines.iter().map(|subroutine| {
        subroutine.index.map_or_else(
            || subroutine.name.clone(),
            |index| format!("{index}: {}", subroutine.name),
        )
    }));
    if markers.is_empty() {
        return output.to_string();
    }

    let lines: Vec<&str> = output.split_inclusive('\n').collect();
    let mut removed = vec![false; lines.len()];
    for (index, line) in lines.iter().enumerate() {
        if !markers.iter().any(|marker| line.trim() == marker) {
            continue;
        }
        let Some(start) = (0..=index).rev().find(|candidate| {
            let trimmed = lines[*candidate].trim_end();
            !trimmed.starts_with(char::is_whitespace) && trimmed.ends_with('{')
        }) else {
            continue;
        };
        let mut depth = 0usize;
        let mut end = start;
        for (candidate, line) in lines.iter().enumerate().skip(start) {
            depth += line.matches('{').count();
            depth = depth.saturating_sub(line.matches('}').count());
            if depth == 0 {
                end = candidate;
                break;
            }
        }
        for removed_line in removed.iter_mut().take(end + 1).skip(start) {
            *removed_line = true;
        }
        if end + 1 < removed.len() && lines[end + 1].trim().is_empty() {
            removed[end + 1] = true;
        }
    }

    lines
        .into_iter()
        .enumerate()
        .filter_map(|(index, line)| (!removed[index]).then_some(line))
        .collect()
}

fn emit_debug_element_counts(
    output: &str,
    program: &workshop_rs::Program,
    catalog: &Catalog,
    hir: &hir::Program,
) -> Result<String, IntegrationError> {
    let report = program.element_count(catalog).map_err(|error| {
        IntegrationError::new(
            "element-count",
            format!("cannot compute Workshop element counts: {error}"),
            None,
        )
    })?;
    let rule_counts: Vec<_> = report.rules.iter().map(debug_rule_count).collect();
    let total = rule_counts.iter().sum::<usize>();
    let mut summary = format!("/* Element count: (total {total})\n\n");
    let mut summary_rules: Vec<_> = report
        .rules
        .iter()
        .zip(&rule_counts)
        .filter(|(_, count)| **count > 1)
        .collect();
    summary_rules.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    for (count, rule_count) in summary_rules {
        let source = count
            .span
            .and_then(|span| {
                hir.files
                    .iter()
                    .find(|file| file.id == span.file.index() as u32)
            })
            .map(|file| file.path.as_str())
            .map(|path| format!(" ({path})"))
            .unwrap_or_default();
        summary.push_str(&format!(
            "{:>5}: rule \"{}\"{source}\n",
            rule_count, count.name
        ));
    }
    summary.push_str("\n*/\n\n");

    let mut annotated = summary;
    let mut rule_index = 0;
    let mut condition_index = 0;
    let mut conditions = Vec::new();
    let mut actions = Vec::new();
    let mut in_actions = false;
    let mut in_conditions = false;
    let mut action_index = 0;
    for line in output.split_inclusive('\n') {
        let trimmed = line.trim();
        if line.starts_with("rule (") {
            if let (Some(rule), Some(rule_count)) =
                (report.rules.get(rule_index), rule_counts.get(rule_index))
            {
                if *rule_count > 1 {
                    let suffix = if *rule_count == 1 {
                        "element"
                    } else {
                        "elements"
                    };
                    annotated.push_str(&format!("//{} {suffix}\n", rule_count));
                }
                conditions.clear();
                collect_element_nodes(
                    &rule.children,
                    workshop_rs::element_count::ElementNodeKind::Condition,
                    &mut conditions,
                );
                actions.clear();
                collect_element_nodes(
                    &rule.children,
                    workshop_rs::element_count::ElementNodeKind::Action,
                    &mut actions,
                );
            }
            rule_index += 1;
            condition_index = 0;
            action_index = 0;
            in_conditions = false;
            in_actions = false;
        } else if trimmed == "conditions {" {
            in_conditions = true;
            in_actions = false;
        } else if trimmed == "actions {" {
            in_conditions = false;
            in_actions = true;
        } else if in_actions && line.starts_with("    }") {
            in_actions = false;
        } else if in_conditions && line.starts_with("    }") {
            in_conditions = false;
        }

        if in_conditions && line.starts_with("        ") && trimmed.ends_with(';') {
            if let Some(condition) = conditions.get(condition_index) {
                let condition_count = debug_condition_count(condition);
                let suffix = if condition_count == 1 {
                    "element"
                } else {
                    "elements"
                };
                annotated.push_str(line.trim_end_matches('\n'));
                annotated.push_str(&format!(" // {condition_count} {suffix}\n"));
                condition_index += 1;
                continue;
            }
        }

        if in_actions && line.starts_with("        ") && trimmed.ends_with(';') {
            if let Some(action) = actions.get(action_index) {
                let action_count = debug_action_local_count(action);
                let suffix = if action_count == 1 {
                    "element"
                } else {
                    "elements"
                };
                annotated.push_str(line.trim_end_matches('\n'));
                annotated.push_str(&format!(" // {action_count} {suffix}\n"));
                action_index += 1;
                continue;
            }
        }
        annotated.push_str(line);
    }
    Ok(annotated)
}

fn debug_value_count(node: &workshop_rs::element_count::ElementCountNode) -> usize {
    let children = node.children.iter().map(debug_value_count).sum::<usize>();
    match node.name.as_str() {
        "number" | "global variable" => 2,
        "localized string" => 2,
        "customString" => 1 + 4usize.saturating_sub(node.children.len()) + children,
        "Team" | "Color" => 1 + children.max(1),
        "array" | "evalOnce" => 2 + children,
        _ if node.children.is_empty() => 1,
        _ => 1 + children,
    }
}

fn debug_condition_count(node: &workshop_rs::element_count::ElementCountNode) -> usize {
    let value_count = node.children.iter().map(debug_value_count).sum::<usize>();
    value_count.saturating_sub(usize::from(node.children.len() > 1))
}

fn debug_action_local_count(node: &workshop_rs::element_count::ElementCountNode) -> usize {
    let values = node
        .children
        .iter()
        .filter(|child| child.kind == workshop_rs::element_count::ElementNodeKind::Value)
        .map(debug_value_count)
        .sum::<usize>();
    let value_arguments = node
        .children
        .iter()
        .filter(|child| child.kind == workshop_rs::element_count::ElementNodeKind::Value)
        .count();
    1 + values.saturating_sub(value_arguments)
}

fn debug_action_count(node: &workshop_rs::element_count::ElementCountNode) -> usize {
    let nested_actions = node
        .children
        .iter()
        .filter(|child| child.kind == workshop_rs::element_count::ElementNodeKind::Action)
        .map(debug_action_count)
        .sum::<usize>();
    debug_action_local_count(node) + nested_actions
}

fn debug_rule_count(node: &workshop_rs::element_count::ElementCountNode) -> usize {
    let children = node
        .children
        .iter()
        .map(|child| match child.kind {
            workshop_rs::element_count::ElementNodeKind::Condition => debug_condition_count(child),
            workshop_rs::element_count::ElementNodeKind::Action => debug_action_count(child),
            workshop_rs::element_count::ElementNodeKind::Rule
            | workshop_rs::element_count::ElementNodeKind::Value => 0,
        })
        .sum::<usize>();
    1 + children
}

fn collect_element_nodes<'a>(
    nodes: &'a [workshop_rs::element_count::ElementCountNode],
    kind: workshop_rs::element_count::ElementNodeKind,
    collected: &mut Vec<&'a workshop_rs::element_count::ElementCountNode>,
) {
    for node in nodes {
        if node.kind == kind {
            collected.push(node);
        }
        collect_element_nodes(&node.children, kind, collected);
    }
}

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

/// The compiler-facing integration object. Construction validates the public
/// manifest/catalog contract once and exposes the pinned catalog identity.
pub struct Compiler {
    catalog: &'static Catalog,
    manifest: &'static Manifest,
    links: LinkReport,
}

impl Compiler {
    pub fn new() -> Result<Self, IntegrationError> {
        let contract = load_compiler_contract()?;
        Ok(Self {
            catalog: &contract.catalog,
            manifest: contract.manifest,
            links: contract.links,
        })
    }

    pub fn catalog_identity(&self) -> CatalogIdentity {
        self.catalog.identity()
    }

    pub fn link_report(&self) -> LinkReport {
        self.links
    }

    /// Lower a resolved OPY HIR program into the canonical Workshop `Program`, validate it
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

        let program = lowering.program;
        program.validate().map_err(|error| {
            IntegrationError::new(
                "workshop-validation",
                error.to_string(),
                workshop_error_span(&error)
                    .and_then(|span| hir_span_from_workshop(span, &expanded_hir)),
            )
        })?;
        workshop_rs::validate::validate_canonical_ids(&program, self.catalog).map_err(|error| {
            IntegrationError::new(
                "catalog-validation",
                error.to_string(),
                workshop_error_span(&error)
                    .and_then(|span| hir_span_from_workshop(span, &expanded_hir)),
            )
        })?;
        let exclude_variables = has_directive(&expanded_hir, "excludeVariablesInCompilation");
        let emitted =
            workshop_rs::emitter::emit(&program, self.catalog, locale).map_err(|error| {
                IntegrationError::new(
                    "workshop-emission",
                    error.to_string(),
                    workshop_error_span(&error)
                        .and_then(|span| hir_span_from_workshop(span, &expanded_hir)),
                )
            })?;
        let emitted = if exclude_variables {
            omit_declaration_sections(&emitted, &program)
        } else {
            emitted
        };
        let emitted = if has_directive(&expanded_hir, "debugElementCount") {
            emit_debug_element_counts(&emitted, &program, self.catalog, &expanded_hir)?
        } else {
            emitted
        };

        Ok(CompilationArtifact {
            wir: program,
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
    /// canonical Workshop types.
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

    /// Compile source and return the canonical Workshop artifact for advanced
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
}

fn workshop_error_span(error: &workshop_rs::WorkshopError) -> Option<workshop_rs::source::Span> {
    match error {
        workshop_rs::WorkshopError::Unknown { span, .. }
        | workshop_rs::WorkshopError::Malformed { span, .. }
        | workshop_rs::WorkshopError::Unsupported { span, .. } => *span,
        workshop_rs::WorkshopError::Catalog(_)
        | workshop_rs::WorkshopError::MissingMapping { .. } => None,
    }
}

fn hir_span_from_workshop(span: workshop_rs::source::Span, hir: &hir::Program) -> Option<HirSpan> {
    let file = span.file.index() as u32;
    hir.files
        .iter()
        .any(|source| source.id == file)
        .then_some(HirSpan {
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

/// A source compile result for ordinary embedding callers.
///
/// The result contains only emitted text and hook output. Callers that need
/// the canonical Workshop `Program` should use the explicit advanced artifact APIs instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOutput {
    /// Workshop text after a declared post-compile hook, if any.
    pub workshop: String,
    /// Workshop text emitted before a declared post-compile hook.
    pub emitted_workshop: String,
    /// Console lines captured while running a declared post-compile hook.
    pub hook_console_output: Vec<String>,
}

/// A validated canonical Workshop `Program` and its emitted Workshop artifact for advanced
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

#[cfg(test)]
mod tests {
    use super::integration::cross_check_manifest;
    use super::{COMPILE_SCHEMA_VERSION, CompileFailureClass, CompileStatus, Compiler};
    use crate::manifest::Manifest;
    use std::path::Path;
    use workshop_rs::catalog::{Catalog, Locale};

    #[test]
    fn catalog_links_are_checked() {
        let compiler = Compiler::new().expect("Workshop contract must load");
        let identity = compiler.catalog_identity();
        assert!(!identity.implementation_version.is_empty());
        assert!(compiler.link_report().catalog_ids_checked > 0);
        assert!(compiler.link_report().domains_checked > 0);
    }

    #[test]
    fn compiler_instances_share_the_verified_contract_concurrently() {
        let instances = std::thread::scope(|scope| {
            let handles = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        let compiler = Compiler::new().expect("Workshop contract must load");
                        (
                            compiler.catalog as *const Catalog as usize,
                            compiler.manifest as *const Manifest as usize,
                            compiler.link_report(),
                        )
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("compiler construction must not panic"))
                .collect::<Vec<_>>()
        });

        let (catalog, manifest, links) = instances[0];
        for (instance_catalog, instance_manifest, instance_links) in instances.into_iter().skip(1) {
            assert_eq!(instance_catalog, catalog);
            assert_eq!(instance_manifest, manifest);
            assert_eq!(instance_links, links);
        }
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
        assert_eq!(report.catalog, compiler.catalog_identity());
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
            .join("tests/fixtures/corpus/synthetic/preprocessing");
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
    fn suppress_warnings_hides_matching_preprocessing_diagnostics() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/corpus/synthetic/preprocessing");
        let outcome = crate::tooling::check(
            concat!(
                "#!suppressWarnings w_already_imported\n",
                "#!include \"shared.opy\"\n",
                "#!include \"shared.opy\"\n",
                "rule \"r\":\n",
                "    @Event global\n",
                "    pass\n",
            ),
            "main.opy",
            &root,
        );
        assert!(outcome.model.is_some());
        assert!(outcome.diagnostics.is_empty());
    }

    #[test]
    fn vertical_slice_preserves_source_files_spans_and_emits_workshop() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar A\nrule \"issue 35 integration\":\n    @Event global\n    A = 1\n    disableInspector()\n",
            "compiler-vertical-slice.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(artifact.emitted.contains("Disable Inspector Recording;"));
        assert_eq!(artifact.catalog_identity, compiler.catalog_identity());
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
        let rule = artifact.wir.rules.first().unwrap();
        assert!(matches!(
            rule.actions.first(),
            Some(workshop_rs::Action::While { .. })
        ));
        assert!(artifact.emitted.contains("While(True);"));
    }

    #[test]
    fn expanded_control_flow_actions_keep_their_originating_spans() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar value = 1\nrule \"if\":\n    @Event global\n    if true:\n        wait(1)\n",
            "control-flow-provenance.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();

        assert_eq!(artifact.wir.action_span(0, 0).unwrap().start.line, 1);
        assert_eq!(
            artifact
                .wir
                .action_argument_span(0, 0, 0)
                .unwrap()
                .start
                .line,
            1
        );
        assert_eq!(artifact.wir.rules[1].actions.len(), 3);
        assert_eq!(artifact.wir.action_span(1, 0).unwrap().start.line, 4);
        assert_eq!(artifact.wir.action_span(1, 1).unwrap().start.line, 5);
        assert_eq!(
            artifact
                .wir
                .action_argument_span(1, 1, 0)
                .unwrap()
                .start
                .line,
            5
        );
        assert_eq!(artifact.wir.action_span(1, 2).unwrap().start.line, 4);
    }

    #[test]
    fn range_argument_provenance_uses_canonical_positions() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar value\nrule \"range\":\n    @Event global\n    for value in range(3):\n        wait(1)\n",
            "range-provenance.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();

        assert_eq!(artifact.wir.action_span(0, 0).unwrap().start.line, 4);
        assert!(artifact.wir.action_argument_span(0, 0, 0).is_none());
        assert_eq!(
            artifact
                .wir
                .action_argument_span(0, 0, 1)
                .unwrap()
                .start
                .line,
            4
        );
        assert!(artifact.wir.action_argument_span(0, 0, 2).is_none());
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
        let subroutine = artifact.wir.subroutines.first().unwrap();
        assert_eq!(subroutine.name, "showStatus");
        assert_eq!(artifact.wir.rules.len(), 2);
        let subroutine_rule = artifact.wir.rules.first().unwrap();
        let workshop_rs::Event::Subroutine(subroutine_name) = &subroutine_rule.event else {
            panic!("expected a subroutine event");
        };
        assert_eq!(subroutine_name, "showStatus");
        assert!(matches!(
            artifact.wir.rules.get(1).unwrap().actions.first(),
            Some(workshop_rs::Action::CallSubroutine { .. })
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
            &artifact.wir.rules.first().unwrap().event,
            workshop_rs::Event::Player {
                kind: workshop_rs::PlayerEventKind::Joined,
                team: workshop_rs::EventTeam::Team1,
                target: workshop_rs::EventTarget::Slot(2),
            }
        ));
        assert!(artifact.emitted.contains("Player Joined Match;"));
    }

    #[test]
    fn hero_event_filters_accept_legacy_aliases() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "rule \"hero\":\n    @Event eachPlayer\n    @Hero soldier\n    disableInspector()\n",
            "hero-filter.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(matches!(
            &artifact.wir.rules.first().unwrap().event,
            workshop_rs::Event::EachPlayerWithFilters {
                target: workshop_rs::EventTarget::Hero(hero),
                ..
            } if hero == "SOLDIER_76"
        ));
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
            .map(|variable| (variable.name.as_str(), variable.index.unwrap()))
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
            .map(|variable| variable.index.unwrap())
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
            .map(|variable| (variable.name.clone(), variable.index.unwrap()))
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
            .map(|variable| (variable.name.as_str(), variable.index.unwrap()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let players = artifact
            .wir
            .player_variables
            .iter()
            .map(|variable| (variable.name.as_str(), variable.index.unwrap()))
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
                .contains("Set Global Variable(scientific, 10000000000);")
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
            .map(|variable| (variable.name.clone(), variable.index.unwrap()))
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
    fn compiler_structure_matches_the_pinned_oracle() {
        let compiler = Compiler::new().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/corpus/synthetic/compiler-structure");
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
            .map(|variable| variable.index.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(indices, vec![0, 1, 2, 3]);
        assert_eq!(artifact.wir.subroutines.first().unwrap().name, "helper");
        assert!(artifact.emitted.contains("[Source] renamed helper"));
        assert!(matches!(
            artifact.wir.rules.get(1).unwrap().event,
            workshop_rs::Event::Player {
                kind: workshop_rs::PlayerEventKind::Joined,
                team: workshop_rs::EventTeam::Team1,
                target: workshop_rs::EventTarget::Slot(2),
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

        let rule = artifact.wir.rules.get(1).unwrap();
        assert!(matches!(
            rule.actions.first(),
            Some(workshop_rs::Action::SetGlobalVariable { variable, .. }) if variable == "g1"
        ));
        assert!(matches!(
            rule.actions.get(7),
            Some(workshop_rs::Action::Call { .. })
        ));
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
        let rule0 = artifact.wir.rules.first().unwrap();
        assert!(rule0.actions.is_empty());
        let rule1 = artifact.wir.rules.get(1).unwrap();
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
            artifact.wir.rules.first().unwrap().name,
            "Initialize global variables"
        );
        assert_eq!(
            artifact.wir.rules.get(1).unwrap().name,
            "Initialize player variables"
        );
        assert_eq!(artifact.wir.rules.get(2).unwrap().name, "main");
        assert!(artifact.emitted.contains("Set Global Variable(j, 5);"));
        assert!(artifact.emitted.contains("Set Global Variable(k, 0);"));
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
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/synthetic/settings");
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
    fn unsupported_output_directives_fail_at_their_source_anchor() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "#!writeToOutputFile\nrule \"r\":\n    @Event global\n    pass\n",
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
    fn strict_optimization_cases_are_preserved_by_native_lowering() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar A = 0\n\n#!optimizeStrict\nrule \"strict\":\n    @Event global\n    print(A + 0)\n    print(A * 0)\n    print(A * 1)\n    print(\"am\" == \"**\")\n",
            "strict.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(artifact.emitted.contains("Add(Global.A, 0)"));
        assert!(artifact.emitted.contains("Multiply(Global.A, 0)"));
        assert!(artifact.emitted.contains("Multiply(Global.A, 1)"));
        assert!(
            artifact
                .emitted
                .contains("Compare(Custom String(\"am\"), ==, Custom String(\"**\"))")
        );
    }

    #[test]
    fn compression_alphabet_policy_uses_a_shared_global() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "globalvar values = compressed([1, 2, 3])\n\n#!useVariableForCompressionAlphabet\nrule \"compression\":\n    @Event global\n    print(values)\n",
            "compression.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(artifact.emitted.contains("127: __compressionAlphabet__"));
        assert!(artifact.emitted.contains("Global.__compressionAlphabet__"));
    }

    #[test]
    fn replacement_directives_lower_when_size_optimization_is_active() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "#!optimizeForSize\n#!replace0ByCapturePercentage\nrule \"r\":\n    @Event global\n    print(0)\n",
            "directives.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(artifact.emitted.contains("Point Capture Percentage"));
    }

    #[test]
    fn debug_element_count_emits_sorted_rule_condition_and_action_comments() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "#!debugElementCount\nglobalvar value\nrule \"small\":\n    @Event global\n    @Condition value == 1\n    print(1)\nrule \"large\":\n    @Event global\n    @Condition value == 1\n    print(1)\n    print(2)\n",
            "debug.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let report = artifact.wir.element_count(compiler.catalog).unwrap();
        let small = report
            .rules
            .iter()
            .find(|rule| rule.name == "small")
            .unwrap();
        let large = report
            .rules
            .iter()
            .find(|rule| rule.name == "large")
            .unwrap();
        let large_summary = artifact.emitted.find("   32: rule \"large\"").unwrap();
        let small_summary = artifact.emitted.find("   18: rule \"small\"").unwrap();
        assert!(large_summary < small_summary);
        assert!(
            artifact
                .emitted
                .starts_with("/* Element count: (total 50)\n")
        );
        assert!(artifact.emitted.contains("//32 elements\nrule (\"large\")"));
        assert!(artifact.emitted.contains("//18 elements\nrule (\"small\")"));
        assert!(
            artifact
                .emitted
                .contains("Global.value == 1; // 3 elements")
        );
        assert_eq!(artifact.emitted.matches(" // 3 elements").count(), 2);
        assert_eq!(artifact.emitted.matches(" // 14 elements").count(), 3);
        let expected_comments = large
            .children
            .iter()
            .chain(&small.children)
            .filter(|node| {
                matches!(
                    node.kind,
                    workshop_rs::element_count::ElementNodeKind::Condition
                        | workshop_rs::element_count::ElementNodeKind::Action
                )
            })
            .count();
        assert_eq!(artifact.emitted.matches(" // ").count(), expected_comments);
    }

    #[test]
    fn setup_and_initialization_directives_change_forward_output() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "#!setupTx\n#!disableInspector\n#!globalvarInitRuleName \"Init globals\"\n#!playervarInitRuleName \"Init players\"\nglobalvar value = 1\nplayervar playerValue = 1\nrule \"r\":\n    @Event global\n    print(\"<fgFF0000FF>ready</fg>\")\n",
            "directives.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(artifact.emitted.contains("OverPy <"));
        assert!(artifact.emitted.contains("Disable inspector"));
        assert!(artifact.emitted.contains("Init globals"));
        assert!(artifact.emitted.contains("Init players"));
        assert!(artifact.emitted.contains("__holygrail__"));
        assert!(
            artifact
                .emitted
                .contains("Custom String(\"{0}fgFF0000FF>ready{0}/fg>\", Global.__holygrail__)")
        );
    }

    #[test]
    fn exclude_variables_directive_omits_variable_declarations() {
        let compiler = Compiler::new().unwrap();
        let hir = crate::compile(
            "#!excludeVariablesInCompilation\nglobalvar value 0\nrule \"r\":\n    @Event global\n    pass\n",
            "directives.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        assert!(!artifact.wir.global_variables.is_empty());
        assert!(
            !artifact.emitted.contains("variables {"),
            "{}",
            artifact.emitted
        );
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
