//! The first OPY-to-Workshop integration boundary.
//!
//! `opy-frontend` remains a standalone OPY/HIR producer. This crate is the
//! consumer-owned compiler layer: it pins the released `workshop-rs` v0.1.8
//! contract, checks the OPY manifest links against the canonical catalog, and
//! lowers the supported OPY program structure into canonical WIR before
//! validation and deterministic Workshop emission.

use std::collections::{BTreeMap, HashMap, HashSet};

use opy_frontend::hir::{self, Expr, RuleEntry, Span as HirSpan, Stmt, default_var_index};
use opy_frontend::manifest::{FunctionKind, Manifest};
use workshop_rs::catalog::{Catalog, CatalogIdentity, Kind, Locale};
use workshop_rs::source::{Position as WorkshopPosition, SourceFile, Span as WorkshopSpan};
use workshop_rs::wir::{self, Action, Event, PlayerEventKind, Program, Value, ValueNode};

/// The exact released dependency contract consumed by this crate.
pub const WORKSHOP_RS_VERSION: &str = "0.1.8";

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
        })
    }
}

/// A validated WIR program and its emitted Workshop artifact.
pub struct CompilationArtifact {
    pub wir: Program,
    pub emitted: String,
    pub catalog_identity: CatalogIdentity,
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
            constants: HashMap::new(),
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
                "custom-game settings lowering is outside #46",
                settings.span,
            ));
        }
        Ok(())
    }

    fn lower_declarations(&mut self) -> Result<(), IntegrationError> {
        let implicit = implicit_default_globals(self.hir);
        for declaration in &self.hir.declarations {
            if let hir::Declaration::GlobalVariable {
                name,
                index: Some(index),
                span,
                ..
            } = declaration
            {
                for (implicit_name, implicit_span) in &implicit {
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
        let implicit_reserved = implicit
            .keys()
            .map(|name| default_var_index(name).expect("implicit default variable names resolve"))
            .collect::<HashSet<_>>();
        let empty = HashSet::new();
        let global_indices = allocate_indices(&globals, &implicit_reserved, "global variable")?;
        let player_indices = allocate_indices(&players, &empty, "player variable")?;
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
                    // Macro definitions are expanded by the frontend; ignored during WIR lowering.
                }
            }
        }

        let mut planned_globals: Vec<(String, u32, Option<HirSpan>, Option<HirSpan>)> =
            declared_globals
                .into_iter()
                .map(|(name, index, span, name_span)| (name.to_string(), index, span, name_span))
                .collect();
        planned_globals.extend(implicit.iter().map(|(name, span)| {
            (
                name.clone(),
                default_var_index(name).expect("implicit default variable names resolve"),
                *span,
                None,
            )
        }));
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

        declared_players.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in declared_players {
            let id = self.wir.player_variables.push(wir::WorkshopVariable {
                name: name.to_string(),
                index: assigned,
                span: self.wir_span(span)?,
                name_span: self.wir_span(name_span)?,
            });
            self.players.insert(name.to_string(), id);
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

        if !global_initializers.is_empty() {
            let mut actions = Vec::with_capacity(global_initializers.len());
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
                name: "Initialize global variables".to_string(),
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
            .map(|expr| self.lower_value(expr))
            .collect::<Result<Vec<_>, _>>()?;
        let mut actions = Vec::new();
        for stmt in &rule.actions {
            if let Some(action) = self.lower_action(stmt)? {
                actions.push(action);
            }
        }
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
        for stmt in body {
            if let Some(action) = self.lower_action(stmt)? {
                actions.push(action);
            }
        }
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

    fn lower_action(&mut self, stmt: &Stmt) -> Result<Option<wir::ActionId>, IntegrationError> {
        match stmt {
            Stmt::Pass { .. } => Ok(None),
            Stmt::Assign {
                target,
                value,
                span,
            } => self.lower_assign(target, value, *span).map(Some),
            Stmt::Expr { expr, span } => match expr.as_ref() {
                Expr::Call { name, args, .. } => {
                    if name == "disableInspector" && args.is_empty() {
                        Ok(Some(self.wir.actions.push(Action::Call {
                            name: "disableInspector".to_string(),
                            args: Vec::new(),
                            span: self.wir_span(*span)?,
                        })))
                    } else if name == "debug" && args.len() == 1 {
                        let val = self.lower_value(&args[0])?;
                        Ok(Some(self.wir.actions.push(Action::Debug {
                            value: val,
                            span: self.wir_span(*span)?,
                        })))
                    } else if name == "print" && args.len() == 1 {
                        let msg = self.lower_value(&args[0])?;
                        Ok(Some(self.wir.actions.push(Action::Print {
                            message: msg,
                            span: self.wir_span(*span)?,
                        })))
                    } else {
                        self.lower_action_call(name, args, *span).map(Some)
                    }
                }
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
                Ok(Some(self.wir.actions.push(Action::CallSubroutine {
                    subroutine,
                    span,
                    callee_span: span,
                })))
            }
            _ => Err(self.unsupported(
                "the statement is not currently representable in canonical WIR",
                stmt.span().copied(),
            )),
        }
    }

    fn lower_assign(
        &mut self,
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<wir::ActionId, IntegrationError> {
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
            Expr::Constant { name, .. } => {
                let const_expr = *self
                    .constants
                    .get(name)
                    .ok_or_else(|| self.unsupported(format!("unknown constant '{name}'"), span))?;
                return self.lower_value(const_expr);
            }
            Expr::Index { array, index, .. } => {
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
                let text_node = self.wir.values.push(ValueNode::new(
                    Value::String(text.clone()),
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
            Expr::Binary {
                op, left, right, ..
            } => match op.as_str() {
                "==" | "!=" | "<" | "<=" | ">" | ">=" => Value::Call {
                    name: op.clone(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "+" => Value::Call {
                    name: "add".to_string(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "-" => Value::Call {
                    name: "subtract".to_string(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "*" => Value::Call {
                    name: "multiply".to_string(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "/" | "//" => Value::Call {
                    name: "divide".to_string(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "%" => Value::Call {
                    name: "modulo".to_string(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "**" | "^" => Value::Call {
                    name: "raiseToPower".to_string(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "and" | "&&" => Value::Call {
                    name: "and".to_string(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "or" | "||" => Value::Call {
                    name: "or".to_string(),
                    args: vec![self.lower_value(left)?, self.lower_value(right)?],
                },
                "in" => Value::Call {
                    name: "arrayContains".to_string(),
                    args: vec![self.lower_value(right)?, self.lower_value(left)?],
                },
                "not in" => {
                    let right_val = self.lower_value(right)?;
                    let left_val = self.lower_value(left)?;
                    let wir_span = self.wir_span(span)?;
                    let contains = self.wir.values.push(ValueNode::new(
                        Value::Call {
                            name: "arrayContains".to_string(),
                            args: vec![right_val, left_val],
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
            },
            Expr::Unary { op, operand, .. } => match op.as_str() {
                "not" | "!" => {
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
                if name == "vect" && args.len() == 3 {
                    Value::Vector {
                        x: self.lower_value(&args[0])?,
                        y: self.lower_value(&args[1])?,
                        z: self.lower_value(&args[2])?,
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

/// Collect the OverPy implicit default global variables (undeclared `A`–`DX`
/// spellings) referenced anywhere in the program, mapped to each name's first
/// source span.
///
/// The pinned OverPy 9.7.10 reference accepts these names without a
/// `globalvar` declaration anywhere a variable may appear and assigns each
/// its fixed Workshop slot (`A` = 0, …, `DX` = 127); a used implicit variable
/// reserves that slot for declared-variable allocation. A name that is also
/// declared keeps the declaration's index, so declared names are excluded
/// here (the declaration wins).
fn implicit_default_globals(hir: &hir::Program) -> BTreeMap<String, Option<HirSpan>> {
    let declared = hir
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            hir::Declaration::GlobalVariable { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut implicit = BTreeMap::new();
    for declaration in &hir.declarations {
        let initializer = match declaration {
            hir::Declaration::GlobalVariable { initializer, .. }
            | hir::Declaration::PlayerVariable { initializer, .. } => initializer.as_ref(),
            hir::Declaration::Constant { value, .. } => Some(value),
            _ => None,
        };
        if let Some(expr) = initializer {
            collect_implicit_expr(expr, &declared, &mut implicit);
        }
    }
    for entry in &hir.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for condition in &rule.conditions {
                    collect_implicit_expr(condition, &declared, &mut implicit);
                }
                collect_implicit_stmts(&rule.actions, &declared, &mut implicit);
            }
            RuleEntry::SubroutineDef { body, .. } => {
                collect_implicit_stmts(body, &declared, &mut implicit);
            }
        }
    }
    implicit
}

fn collect_implicit_stmts(
    statements: &[Stmt],
    declared: &HashSet<&str>,
    implicit: &mut BTreeMap<String, Option<HirSpan>>,
) {
    for statement in statements {
        match statement {
            Stmt::Expr { expr, .. } => collect_implicit_expr(expr, declared, implicit),
            Stmt::Assign { target, value, .. } => {
                collect_implicit_expr(target, declared, implicit);
                collect_implicit_expr(value, declared, implicit);
            }
            Stmt::If {
                branches, r#else, ..
            } => {
                for branch in branches {
                    collect_implicit_expr(&branch.condition, declared, implicit);
                    collect_implicit_stmts(&branch.body, declared, implicit);
                }
                if let Some(default_body) = r#else {
                    collect_implicit_stmts(default_body, declared, implicit);
                }
            }
            Stmt::For {
                variable,
                iterable,
                body,
                ..
            } => {
                collect_implicit_expr(variable, declared, implicit);
                collect_implicit_expr(iterable, declared, implicit);
                collect_implicit_stmts(body, declared, implicit);
            }
            Stmt::While {
                condition, body, ..
            }
            | Stmt::DoWhile {
                condition, body, ..
            } => {
                collect_implicit_expr(condition, declared, implicit);
                collect_implicit_stmts(body, declared, implicit);
            }
            Stmt::Switch {
                value,
                cases,
                r#default,
                ..
            } => {
                collect_implicit_expr(value, declared, implicit);
                for case in cases {
                    collect_implicit_expr(&case.value, declared, implicit);
                    collect_implicit_stmts(&case.body, declared, implicit);
                }
                if let Some(default) = r#default {
                    collect_implicit_stmts(default, declared, implicit);
                }
            }
            Stmt::Break { .. } | Stmt::CallSubroutine { .. } | Stmt::Pass { .. } => {}
        }
    }
}

fn collect_implicit_expr(
    expr: &Expr,
    declared: &HashSet<&str>,
    implicit: &mut BTreeMap<String, Option<HirSpan>>,
) {
    match expr {
        Expr::GlobalVar { name, span } => {
            if !declared.contains(name.as_str()) && default_var_index(name).is_some() {
                implicit.entry(name.clone()).or_insert(*span);
            }
        }
        Expr::Array { elements, .. } => {
            for element in elements {
                collect_implicit_expr(element, declared, implicit);
            }
        }
        Expr::Dict { entries, .. } => {
            for entry in entries {
                collect_implicit_expr(&entry.key, declared, implicit);
                collect_implicit_expr(&entry.value, declared, implicit);
            }
        }
        Expr::Comprehension {
            element,
            iterable,
            condition,
            ..
        } => {
            collect_implicit_expr(element, declared, implicit);
            collect_implicit_expr(iterable, declared, implicit);
            if let Some(condition) = condition {
                collect_implicit_expr(condition, declared, implicit);
            }
        }
        Expr::Lambda { body, .. } => collect_implicit_expr(body, declared, implicit),
        Expr::Vector { x, y, z, .. } => {
            collect_implicit_expr(x, declared, implicit);
            collect_implicit_expr(y, declared, implicit);
            collect_implicit_expr(z, declared, implicit);
        }
        Expr::PlayerVar { player, .. } => collect_implicit_expr(player, declared, implicit),
        Expr::Member { receiver, .. } => collect_implicit_expr(receiver, declared, implicit),
        Expr::Call { args, .. } | Expr::MacroCall { args, .. } => {
            for arg in args {
                collect_implicit_expr(arg, declared, implicit);
            }
        }
        Expr::ReceiverCall { receiver, args, .. } => {
            collect_implicit_expr(receiver, declared, implicit);
            for arg in args {
                collect_implicit_expr(arg, declared, implicit);
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_implicit_expr(left, declared, implicit);
            collect_implicit_expr(right, declared, implicit);
        }
        Expr::Unary { operand, .. } => collect_implicit_expr(operand, declared, implicit),
        Expr::Index { array, index, .. } => {
            collect_implicit_expr(array, declared, implicit);
            collect_implicit_expr(index, declared, implicit);
        }
        Expr::Format { args, .. } => {
            for arg in args {
                collect_implicit_expr(arg, declared, implicit);
            }
        }
        // Leaf expressions carry no variable references.
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Null { .. }
        | Expr::StringModifier { .. }
        | Expr::Local { .. }
        | Expr::Enum { .. }
        | Expr::EventPlayer { .. }
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
        _ => false,
    }
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
        "/" | "//" => Some(wir::ModifyOp::Divide),
        "%" => Some(wir::ModifyOp::Modulo),
        "**" | "^" => Some(wir::ModifyOp::RaiseToPower),
        _ => None,
    }
}

fn modify_catalog_name_from_str(op: &str) -> Option<&'static str> {
    match op {
        "+" => Some("add"),
        "-" => Some("subtract"),
        "*" => Some("multiply"),
        "/" | "//" => Some("divide"),
        "%" => Some("modulo"),
        "**" | "^" => Some("raiseToPower"),
        _ => None,
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
        assert_eq!(artifact.catalog_identity.implementation_version, "0.1.8");
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
            "rule \"unsupported\":\n    @Event global\n    while true:\n        disableInspector()\n",
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
        let hir = opy_frontend::compile(
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
        let hir = opy_frontend::compile(
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
    fn power_augmented_assignment_lowers_from_source() {
        let compiler = Compiler::new().unwrap();
        let hir = opy_frontend::compile(
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
    fn unsupported_primitive_lowering_is_stable_and_source_attributed() {
        let compiler = Compiler::new().unwrap();
        let hir = opy_frontend::compile(
            "globalvar total\nrule \"negative\":\n    @Event global\n    total = {\"a\": 1, \"b\": 2}[\"a\"]\n",
            "negative.opy",
            Path::new("."),
        )
        .unwrap();
        let error = match compiler.compile_hir(&hir) {
            Ok(_) => panic!("dict primitive lowering unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
        assert!(error.diagnostic.message.contains("dict"));
        assert_eq!(error.diagnostic.span.unwrap().start.line, 4);
    }

    #[test]
    fn auto_allocation_fills_free_slots_below_early_explicit_indices() {
        let compiler = Compiler::new().unwrap();
        let hir = opy_frontend::compile(
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
        let hir = opy_frontend::compile(
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

    #[test]
    fn issue_40_oracle_fixture_and_wir_lowering_agree() {
        let compiler = Compiler::new().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compatibility/fixtures/synthetic/issue-40-structural");
        let source = std::fs::read_to_string(fixture.join("source.opy")).unwrap();
        let hir = opy_frontend::compile(&source, "source.opy", &fixture).unwrap();
        let artifact = compiler.compile_hir(&hir).unwrap();
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture.join("oracle.json")).unwrap())
                .unwrap();
        let oracle_workshop = oracle["compile"]["workshop"].as_str().unwrap();

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
        let hir = opy_frontend::compile(
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
        let hir = opy_frontend::compile(
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
        let hir = opy_frontend::compile(
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
        let hir = opy_frontend::compile(
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
}
