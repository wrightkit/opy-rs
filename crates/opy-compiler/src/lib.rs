//! The first OPY-to-Workshop integration boundary.
//!
//! `opy-frontend` remains a standalone OPY/HIR producer. This crate is the
//! consumer-owned compiler layer: it pins the released `workshop-rs` v0.1.1
//! contract, checks the OPY manifest links against the canonical catalog, and
//! lowers the small validated vertical slice into canonical WIR before
//! validation and deterministic Workshop emission.

use std::collections::HashMap;
use std::path::Path;

use opy_frontend::hir::{self, Expr, RuleEntry, Span as HirSpan, Stmt};
use opy_frontend::manifest::{FunctionKind, Manifest};
use workshop_rs::catalog::{Catalog, CatalogIdentity, Kind, Locale};
use workshop_rs::source::{Position as WorkshopPosition, SourceFile, Span as WorkshopSpan};
use workshop_rs::wir::{self, Action, Event, Program, Value, ValueNode};

/// The exact released dependency contract consumed by this crate.
pub const WORKSHOP_RS_VERSION: &str = "0.1.1";

/// A source-attributed integration diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationDiagnostic {
    pub code: String,
    pub message: String,
    pub span: Option<HirSpan>,
}

impl IntegrationDiagnostic {
    fn new(code: impl Into<String>, message: impl Into<String>, span: Option<HirSpan>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            span,
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
}

impl std::fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.diagnostic.code, self.diagnostic.message)
    }
}

impl std::error::Error for IntegrationError {}

/// Errors from the source convenience API, retaining the frontend boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    Frontend(opy_frontend::FrontendError),
    Integration(IntegrationError),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frontend(error) => error.fmt(f),
            Self::Integration(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CompileError {}

/// Results of the manifest-to-catalog cross-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkReport {
    pub catalog_ids_checked: usize,
    pub domains_checked: usize,
}

/// Cross-check every OPY manifest `catalogId` and domain identity against the
/// canonical Workshop catalog. No local catalog copy or spelling allowlist is
/// involved.
pub fn cross_check_manifest(
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

    /// Compile source through the standalone frontend and this integration
    /// layer. The frontend dependency remains one-way: it does not know this
    /// crate or workshop-rs.
    pub fn compile_source(
        &self,
        source: &str,
        main_path: &str,
        root: &Path,
    ) -> Result<CompilationArtifact, CompileError> {
        let hir = opy_frontend::compile(source, main_path, root).map_err(CompileError::Frontend)?;
        self.compile_hir(&hir).map_err(CompileError::Integration)
    }

    /// Lower a resolved OPY HIR program into canonical WIR, validate it
    /// against the canonical catalog, and emit deterministic en-US Workshop.
    pub fn compile_hir(&self, hir: &hir::Program) -> Result<CompilationArtifact, IntegrationError> {
        let mut lowering = Lowering::new(self, hir)?;
        lowering.copy_files();
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
            workshop_rs::emitter::emit(&lowering.wir, &self.catalog, &Locale::new("en-US"))
                .map_err(|error| {
                    let span = workshop_error_span(&error)
                        .and_then(|span| lowering.hir_span_from_workshop(span));
                    IntegrationError::new("workshop-emission", error.to_string(), span)
                })?;

        Ok(CompilationArtifact {
            wir: lowering.wir,
            emitted,
            catalog_identity: self.catalog.identity(),
            link_report: self.links,
        })
    }
}

/// A validated WIR program and its emitted Workshop artifact.
pub struct CompilationArtifact {
    pub wir: Program,
    pub emitted: String,
    pub catalog_identity: CatalogIdentity,
    pub link_report: LinkReport,
}

struct Lowering<'a> {
    compiler: &'a Compiler,
    hir: &'a hir::Program,
    wir: Program,
    files: HashMap<u32, workshop_rs::source::FileId>,
    wir_to_hir_files: Vec<u32>,
    globals: HashMap<String, wir::GlobalVarId>,
    players: HashMap<String, wir::PlayerVarId>,
}

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
        })
    }

    fn copy_files(&mut self) {
        for file in &self.hir.files {
            let id = self.wir.files.push(SourceFile::new(file.path.clone()));
            self.files.insert(file.id, id);
            self.wir_to_hir_files.push(file.id);
        }
    }

    fn lower_declarations(&mut self) -> Result<(), IntegrationError> {
        let mut next_global = 0;
        let mut next_player = 0;
        for declaration in &self.hir.declarations {
            match declaration {
                hir::Declaration::GlobalVariable {
                    name,
                    index,
                    span,
                    name_span,
                    initializer,
                } => {
                    if initializer.is_some() {
                        return Err(self.unsupported(
                            "global variable initializers are outside the #35 vertical slice",
                            *span,
                        ));
                    }
                    let assigned = index.unwrap_or(next_global);
                    next_global = next_global.max(assigned.saturating_add(1));
                    let id = self.wir.global_variables.push(wir::WorkshopVariable {
                        name: name.clone(),
                        index: assigned,
                        span: self.wir_span(*span)?,
                        name_span: self.wir_span(*name_span)?,
                    });
                    self.globals.insert(name.clone(), id);
                }
                hir::Declaration::PlayerVariable {
                    name,
                    index,
                    span,
                    name_span,
                    initializer,
                } => {
                    if initializer.is_some() {
                        return Err(self.unsupported(
                            "player variable initializers are outside the #35 vertical slice",
                            *span,
                        ));
                    }
                    let assigned = index.unwrap_or(next_player);
                    next_player = next_player.max(assigned.saturating_add(1));
                    let id = self.wir.player_variables.push(wir::WorkshopVariable {
                        name: name.clone(),
                        index: assigned,
                        span: self.wir_span(*span)?,
                        name_span: self.wir_span(*name_span)?,
                    });
                    self.players.insert(name.clone(), id);
                }
                hir::Declaration::Subroutine { .. }
                | hir::Declaration::Constant { .. }
                | hir::Declaration::Macro { .. } => {}
            }
        }
        Ok(())
    }

    fn lower_rules(&mut self) -> Result<(), IntegrationError> {
        for entry in &self.hir.rules {
            match entry {
                RuleEntry::Rule(rule) => self.lower_rule(rule)?,
                RuleEntry::SubroutineDef { span, .. } => {
                    return Err(self.unsupported(
                        "subroutine rule lowering is outside the #35 vertical slice",
                        *span,
                    ));
                }
            }
        }
        Ok(())
    }

    fn lower_rule(&mut self, rule: &hir::Rule) -> Result<(), IntegrationError> {
        let event = match rule.event.name.as_str() {
            "global" => Event::Global,
            "eachPlayer" => Event::EachPlayer,
            _ => {
                return Err(self.unsupported(
                    format!(
                        "event '{}' is outside the #35 vertical slice",
                        rule.event.name
                    ),
                    rule.event.span,
                ));
            }
        };
        let conditions = rule
            .conditions
            .iter()
            .map(|expr| self.lower_value(expr))
            .collect::<Result<Vec<_>, _>>()?;
        let actions = rule
            .actions
            .iter()
            .map(|stmt| self.lower_action(stmt))
            .collect::<Result<Vec<_>, _>>()?;
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

    fn lower_action(&mut self, stmt: &Stmt) -> Result<wir::ActionId, IntegrationError> {
        match stmt {
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                let Expr::GlobalVar {
                    name,
                    span: target_span,
                } = target.as_ref()
                else {
                    return Err(self.unsupported(
                        "only global-variable assignment is in the #35 vertical slice",
                        *span,
                    ));
                };
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                let value = self.lower_value(value)?;
                Ok(self.wir.actions.push(Action::SetGlobalVariable {
                    variable,
                    value,
                    span: self.wir_span(*span)?,
                    target_span: self.wir_span(*target_span)?,
                }))
            }
            Stmt::Expr { expr, span } => {
                let Expr::Call { name, args, .. } = expr.as_ref() else {
                    return Err(self.unsupported(
                        "only builtin action calls are in the #35 vertical slice",
                        *span,
                    ));
                };
                self.lower_action_call(name, args, *span)
            }
            _ => Err(self.unsupported(
                "the statement is outside the #35 vertical slice",
                stmt.span().copied(),
            )),
        }
    }

    fn lower_action_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
        let function = self
            .compiler
            .manifest
            .resolve_function(name)
            .ok_or_else(|| self.unsupported(format!("unknown action '{name}'"), span))?;
        if !matches!(function.kind, FunctionKind::Action) {
            return Err(self.unsupported(format!("'{name}' is not a generic OPY action"), span));
        }
        let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
            self.unsupported(
                format!(
                    "action '{}' requires a special lowering not in #35",
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

    fn lower_value(&mut self, expr: &Expr) -> Result<wir::ValueId, IntegrationError> {
        let span = expr.span().copied();
        let value = match expr {
            Expr::Number { value, text, .. } => Value::Number {
                value: *value,
                text: text.clone(),
            },
            Expr::String { value, .. } => Value::String(value.clone()),
            Expr::Bool { value, .. } => Value::Bool(*value),
            Expr::Null { .. } => Value::Null,
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
            Expr::Enum {
                value_type, value, ..
            } => Value::Enum {
                value_type: value_type.clone(),
                value: value.clone(),
            },
            Expr::Array { elements, .. } => Value::Array(
                elements
                    .iter()
                    .map(|element| self.lower_value(element))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Expr::Vector { x, y, z, .. } => Value::Vector {
                x: self.lower_value(x)?,
                y: self.lower_value(y)?,
                z: self.lower_value(z)?,
            },
            Expr::Call { name, args, .. } => {
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
                            "value '{}' requires a special lowering not in #35",
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
            Expr::Binary {
                op, left, right, ..
            } => Value::Call {
                name: op.clone(),
                args: vec![self.lower_value(left)?, self.lower_value(right)?],
            },
            Expr::Unary { op, operand, .. } => Value::Call {
                name: op.clone(),
                args: vec![self.lower_value(operand)?],
            },
            _ => {
                return Err(self.unsupported(
                    format!(
                        "expression '{}' is outside the #35 vertical slice",
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
    use super::{Compiler, WORKSHOP_RS_VERSION, cross_check_manifest};
    use opy_frontend::manifest::Manifest;
    use std::path::Path;
    use workshop_rs::catalog::Catalog;

    #[test]
    fn public_contract_is_pinned_and_manifest_links_are_checked() {
        let compiler = Compiler::new().expect("released workshop contract must load");
        let identity = compiler.catalog_identity();
        assert_eq!(identity.implementation_version, WORKSHOP_RS_VERSION);
        assert!(compiler.link_report().catalog_ids_checked > 0);
        assert!(compiler.link_report().domains_checked > 0);
    }

    #[test]
    fn vertical_slice_preserves_source_files_spans_and_emits_workshop() {
        let compiler = Compiler::new().unwrap();
        let hir = opy_frontend::compile(
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
        assert_eq!(artifact.catalog_identity.implementation_version, "0.1.1");
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
    fn unsupported_lowering_remains_source_attributed() {
        let compiler = Compiler::new().unwrap();
        let hir = opy_frontend::compile(
            "rule \"unsupported\":\n    @Event global\n    pass\n",
            "unsupported.opy",
            Path::new("."),
        )
        .unwrap();
        let error = match compiler.compile_hir(&hir) {
            Ok(_) => panic!("unsupported lowering unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
        assert_eq!(error.diagnostic.span.unwrap().start.line, 3);
    }
}
