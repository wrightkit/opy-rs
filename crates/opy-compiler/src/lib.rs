//! The first OPY-to-Workshop integration boundary.
//!
//! `opy-frontend` remains a standalone OPY/HIR producer. This crate is the
//! consumer-owned compiler layer: it pins the released `workshop-rs` v0.1.1
//! contract, checks the OPY manifest links against the canonical catalog, and
//! lowers the supported OPY program structure into canonical WIR before
//! validation and deterministic Workshop emission.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use opy_frontend::hir::{self, Expr, RuleEntry, Span as HirSpan, Stmt};
use opy_frontend::manifest::{FunctionKind, Manifest};
use workshop_rs::catalog::{Catalog, CatalogIdentity, Kind, Locale};
use workshop_rs::source::{Position as WorkshopPosition, SourceFile, Span as WorkshopSpan};
use workshop_rs::wir::{self, Action, Event, PlayerEventKind, Program, Value, ValueNode};

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
        lowering.copy_files()?;
        lowering.reject_unsupported_metadata()?;
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
    subroutines: HashMap<String, wir::SubroutineId>,
    defined_subroutines: HashSet<wir::SubroutineId>,
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
            subroutines: HashMap::new(),
            defined_subroutines: HashSet::new(),
        })
    }

    fn copy_files(&mut self) -> Result<(), IntegrationError> {
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

    fn reject_unsupported_metadata(&self) -> Result<(), IntegrationError> {
        if let Some(settings) = &self.hir.settings {
            return Err(self.unsupported(
                "custom-game settings lowering is outside #40",
                settings.span,
            ));
        }
        Ok(())
    }

    fn lower_declarations(&mut self) -> Result<(), IntegrationError> {
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
        let global_indices = allocate_indices(&globals, "global variable")?;
        let player_indices = allocate_indices(&players, "player variable")?;
        let subroutine_indices = allocate_indices(&subroutines, "subroutine")?;
        let mut global_index = 0;
        let mut player_index = 0;
        let mut subroutine_index = 0;
        for declaration in &self.hir.declarations {
            match declaration {
                hir::Declaration::GlobalVariable {
                    name,
                    index: _,
                    span,
                    name_span,
                    initializer,
                } => {
                    if initializer.is_some() {
                        return Err(
                            self.unsupported("global variable initializers are outside #40", *span)
                        );
                    }
                    let assigned = global_indices[global_index];
                    global_index += 1;
                    let id = self.wir.global_variables.push(wir::WorkshopVariable {
                        name: name.clone(),
                        index: assigned,
                        span: self.wir_span(*span)?,
                        name_span: self.wir_span(*name_span)?,
                    });
                    if self.globals.insert(name.clone(), id).is_some() {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate global variable '{name}'"),
                            *span,
                        ));
                    }
                }
                hir::Declaration::PlayerVariable {
                    name,
                    index: _,
                    span,
                    name_span,
                    initializer,
                } => {
                    if initializer.is_some() {
                        return Err(
                            self.unsupported("player variable initializers are outside #40", *span)
                        );
                    }
                    let assigned = player_indices[player_index];
                    player_index += 1;
                    let id = self.wir.player_variables.push(wir::WorkshopVariable {
                        name: name.clone(),
                        index: assigned,
                        span: self.wir_span(*span)?,
                        name_span: self.wir_span(*name_span)?,
                    });
                    if self.players.insert(name.clone(), id).is_some() {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate player variable '{name}'"),
                            *span,
                        ));
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
                    let id = self.wir.subroutines.push(wir::WorkshopSubroutine {
                        name: name.clone(),
                        index: assigned,
                        span: self.wir_span(*span)?,
                        name_span: self.wir_span(*name_span)?,
                    });
                    if self.subroutines.insert(name.clone(), id).is_some() {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate subroutine '{name}'"),
                            *span,
                        ));
                    }
                }
                hir::Declaration::Constant { name, span, .. } => {
                    return Err(self.unsupported(
                        format!(
                            "constant declaration '{name}' is not representable in canonical WIR"
                        ),
                        *span,
                    ));
                }
                hir::Declaration::Macro { name, span, .. } => {
                    return Err(self.unsupported(
                        format!("macro declaration '{name}' is not representable in canonical WIR"),
                        *span,
                    ));
                }
            }
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
        if let Some(declaration) = self.wir.subroutines.get_mut(subroutine) {
            declaration.name = name.to_string();
        }
        if let Some(existing) = self.subroutines.get(name) {
            if *existing != subroutine {
                return Err(self.unsupported(
                    format!(
                        "subroutine presentation name '{name}' collides with another subroutine"
                    ),
                    name_span.or(span),
                ));
            }
        } else {
            self.subroutines.insert(name.to_string(), subroutine);
        }
        let actions = body
            .iter()
            .map(|stmt| self.lower_action(stmt))
            .collect::<Result<Vec<_>, _>>()?;
        self.wir.rules.push(wir::Rule {
            name: format!("Subroutine {name}"),
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
                "Event" | "Condition" | "Team" | "Slot" | "Hero" | "Disabled" => {}
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
                "Name" => {}
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
                        "only global-variable assignment is currently representable in canonical WIR",
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
                        "only builtin action calls are currently representable in canonical WIR",
                        *span,
                    ));
                };
                self.lower_action_call(name, args, *span)
            }
            Stmt::CallSubroutine { name, span } => {
                let subroutine = *self.subroutines.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown subroutine '{name}'"), *span)
                })?;
                let span = self.wir_span(*span)?;
                Ok(self.wir.actions.push(Action::CallSubroutine {
                    subroutine,
                    span,
                    callee_span: span,
                }))
            }
            _ => Err(self.unsupported(
                "the statement is not currently representable in canonical WIR",
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
                    "action '{}' requires a special lowering not in #40",
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
                            "value '{}' requires a special lowering not in #40",
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

fn allocate_indices(
    entries: &[(Option<u32>, Option<HirSpan>)],
    kind: &str,
) -> Result<Vec<u32>, IntegrationError> {
    let mut reserved = HashSet::new();
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
        next = next.max(assigned.saturating_add(1));
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

    #[test]
    fn structural_subroutines_lower_to_canonical_wir() {
        let compiler = Compiler::new().unwrap();
        let hir = opy_frontend::compile(
            "globalvar score\nsubroutine showStatus\ndef showStatus():\n    @Name \"Friendly\"\n    disableInspector()\nrule \"caller\":\n    @Event global\n    showStatus()\n",
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
        assert_eq!(subroutine.name, "Friendly");
        assert_eq!(subroutine.index, 0);
        assert_eq!(subroutine.name_span.unwrap().start.line, 2);
        assert_eq!(artifact.wir.rules.len(), 2);
        assert!(matches!(
            artifact
                .wir
                .rules
                .get(workshop_rs::wir::RuleId::from_index(0))
                .unwrap()
                .event,
            workshop_rs::wir::Event::Subroutine(_)
        ));
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
        let hir = opy_frontend::compile(
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
        let hir = opy_frontend::compile(
            "globalvar first\nglobalvar reserved 0\nglobalvar next\nrule \"indices\":\n    @Event global\n    disableInspector()\n",
            "indices.opy",
            Path::new("."),
        )
        .unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let indices = artifact
            .wir
            .global_variables
            .iter()
            .map(|variable| variable.index)
            .collect::<Vec<_>>();
        assert_eq!(indices, vec![1, 0, 2]);
    }

    #[test]
    fn unsupported_rule_metadata_is_explicit_and_source_attributed() {
        let compiler = Compiler::new().unwrap();
        let hir = opy_frontend::compile(
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
}
