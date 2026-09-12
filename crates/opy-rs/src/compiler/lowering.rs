use super::*;

type ValueId = usize;
type ActionId = usize;
type GlobalVarId = usize;
type PlayerVarId = usize;
type SubroutineId = usize;
use workshop_rs::{Action, Event, EventTarget, EventTeam, ModifyOp, PlayerEventKind, Value};

pub(crate) struct Lowering<'a> {
    compiler: &'a Compiler,
    hir: &'a hir::Program,
    pub(super) program: Program,
    values: Vec<Value>,
    actions: Vec<Action>,
    globals: HashMap<String, GlobalVarId>,
    global_names: Vec<String>,
    players: HashMap<String, PlayerVarId>,
    player_names: Vec<String>,
    subroutines: HashMap<String, SubroutineId>,
    subroutine_names: Vec<String>,
    constants: HashMap<String, &'a Expr>,
    defined_subroutines: HashSet<SubroutineId>,
    array_bindings: Vec<ArrayBinding>,
    current_rule_conditions: Option<Vec<ValueId>>,
    visible_labels: Vec<HashSet<String>>,
    deferred_gotos: Vec<(ActionId, String, Option<HirSpan>)>,
    outer_goto_targets: Vec<String>,
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

enum DeleteAssignment {
    Global(GlobalVarId),
    Player {
        player: ValueId,
        variable: PlayerVarId,
    },
}

type SwitchBreak = (usize, HirSpan);
type LoweredSwitchBody = (Vec<ActionId>, Option<SwitchBreak>);
type LoweredSwitchArm<'a> = (Option<&'a Expr>, Vec<ActionId>, Option<SwitchBreak>);

fn pure_continue_conditions(statement: &Stmt) -> Option<Vec<&Expr>> {
    match statement {
        Stmt::Continue { .. } => Some(Vec::new()),
        Stmt::If {
            branches,
            r#else: None,
            ..
        } if branches.len() == 1 && branches[0].body.len() == 1 => {
            let mut conditions = pure_continue_conditions(&branches[0].body[0])?;
            conditions.insert(0, &branches[0].condition);
            Some(conditions)
        }
        _ => None,
    }
}

fn pure_goto_conditions(statement: &Stmt) -> Option<(Vec<&Expr>, &str)> {
    match statement {
        Stmt::Goto {
            label: Some(label),
            offset: None,
            rule_start: false,
            ..
        } => Some((Vec::new(), label.as_str())),
        Stmt::If {
            branches,
            r#else: None,
            ..
        } if branches.len() == 1 && branches[0].body.len() == 1 => {
            let (mut conditions, label) = pure_goto_conditions(&branches[0].body[0])?;
            conditions.insert(0, &branches[0].condition);
            Some((conditions, label))
        }
        _ => None,
    }
}

fn contains_loop_continue(statement: &Stmt) -> bool {
    match statement {
        Stmt::Continue { .. } => true,
        Stmt::If {
            branches, r#else, ..
        } => {
            branches
                .iter()
                .any(|branch| branch.body.iter().any(contains_loop_continue))
                || r#else
                    .as_ref()
                    .is_some_and(|body| body.iter().any(contains_loop_continue))
        }
        Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => false,
        _ => false,
    }
}

impl<'a> Lowering<'a> {
    pub(super) fn new(
        compiler: &'a Compiler,
        hir: &'a hir::Program,
    ) -> Result<Self, IntegrationError> {
        Ok(Self {
            compiler,
            hir,
            program: Program::default(),
            values: Vec::new(),
            actions: Vec::new(),
            globals: HashMap::new(),
            global_names: Vec::new(),
            players: HashMap::new(),
            player_names: Vec::new(),
            subroutines: HashMap::new(),
            subroutine_names: Vec::new(),
            constants: HashMap::new(),
            defined_subroutines: HashSet::new(),
            array_bindings: Vec::new(),
            current_rule_conditions: None,
            visible_labels: Vec::new(),
            deferred_gotos: Vec::new(),
            outer_goto_targets: Vec::new(),
        })
    }

    pub(super) fn copy_files(&mut self) -> Result<(), IntegrationError> {
        for file in &self.hir.files {
            self.program
                .add_file(workshop_rs::source::SourceFile::new(file.path.clone()));
        }
        let settings_constants = self
            .hir
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::Constant { name, value, .. } => {
                    Some((name.clone(), value.as_ref()))
                }
                _ => None,
            })
            .collect();
        self.program.settings = self.hir.settings.clone().map(|settings| {
            super::settings::convert_settings(super::settings::expand_settings_constants(
                settings,
                &settings_constants,
            ))
        });
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

    pub(super) fn lower_declarations(&mut self) -> Result<(), IntegrationError> {
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
                hir::Declaration::Macro { .. } => {}
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
            let _ = (span, name_span);
            let id = self.global_names.len();
            self.global_names.push(name.clone());
            self.globals.insert(name.clone(), id);
            self.program
                .global_variables
                .push(workshop_rs::Variable::with_index(name, assigned));
            self.program
                .set_global_variable_spans(
                    id,
                    self.workshop_span(span)?,
                    self.workshop_span(name_span)?,
                )
                .map_err(|error| {
                    IntegrationError::new("provenance", error.to_string(), span.or(name_span))
                })?;
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
            let _ = (span, name_span);
            let id = self.player_names.len();
            self.player_names.push(name.clone());
            self.players.insert(name, id);
            self.program
                .player_variables
                .push(workshop_rs::Variable::with_index(
                    self.player_names[id].clone(),
                    assigned,
                ));
            self.program
                .set_player_variable_spans(
                    id,
                    self.workshop_span(span)?,
                    self.workshop_span(name_span)?,
                )
                .map_err(|error| {
                    IntegrationError::new("provenance", error.to_string(), span.or(name_span))
                })?;
        }

        declared_subroutines.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in declared_subroutines {
            let _ = (span, name_span);
            let id = self.subroutine_names.len();
            self.subroutine_names.push(name.to_string());
            self.subroutines.insert(name.to_string(), id);
            self.program
                .subroutines
                .push(workshop_rs::Subroutine::with_index(name, assigned));
            self.program
                .set_subroutine_spans(
                    id,
                    self.workshop_span(span)?,
                    self.workshop_span(name_span)?,
                )
                .map_err(|error| {
                    IntegrationError::new("provenance", error.to_string(), span.or(name_span))
                })?;
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
                Ok(self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value: self.value(value).clone(),
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
            for (name, init_expr, _span, _target_span) in global_initializers {
                let variable = *self.globals.get(name).expect("declared global is created");
                let value = self.lower_value(init_expr)?;
                actions.push(self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value: self.value(value).clone(),
                }));
            }
            self.program.rules.push(workshop_rs::Rule {
                name: self.global_initializer_rule_name(),
                disabled: false,
                event: workshop_rs::Event::Global,
                conditions: Vec::new(),
                actions: self.public_actions(&actions),
            });
        }

        if !player_initializers.is_empty() {
            let mut actions = Vec::with_capacity(player_initializers.len());
            for (name, init_expr, _span, _target_span) in player_initializers {
                let variable = *self
                    .players
                    .get(name)
                    .expect("declared player variable is created");
                let player = self.push_value(Value::EventPlayer);
                let value = self.lower_value(init_expr)?;
                actions.push(self.push_action(Action::SetPlayerVariable {
                    player: self.value(player).clone(),
                    variable: self.player_names[variable].clone(),
                    value: self.value(value).clone(),
                }));
            }
            self.program.rules.push(workshop_rs::Rule {
                name: "Initialize player variables".to_string(),
                disabled: false,
                event: workshop_rs::Event::EachPlayer,
                conditions: Vec::new(),
                actions: self.public_actions(&actions),
            });
        }

        Ok(())
    }

    pub(super) fn lower_rules(&mut self) -> Result<(), IntegrationError> {
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
        if rule.disabled {
            return Ok(());
        }
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
        let rule_index = self.program.rules.len();
        self.program.rules.push(workshop_rs::Rule {
            name: rule.name.clone(),
            disabled: rule.disabled,
            event,
            conditions: conditions
                .iter()
                .map(|value| workshop_rs::Condition::new(self.value(*value).clone()))
                .collect(),
            actions: self.public_actions(&actions),
        });
        self.set_rule_provenance(
            rule_index,
            rule.span,
            rule.conditions.iter().map(|expr| expr.span().copied()),
            actions.iter().enumerate().map(|(index, _)| {
                rule.actions
                    .get(index)
                    .and_then(|statement| statement.span().copied())
            }),
        )?;
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
        let rule_index = self.program.rules.len();
        self.program.rules.push(workshop_rs::Rule {
            name: self.subroutine_rule_name(name),
            disabled: false,
            event: Event::Subroutine(self.subroutine_names[subroutine].clone()),
            conditions: Vec::new(),
            actions: self.public_actions(&actions),
        });
        self.set_rule_provenance(
            rule_index,
            span,
            std::iter::empty(),
            actions.iter().enumerate().map(|(index, _)| {
                body.get(index)
                    .and_then(|statement| statement.span().copied())
            }),
        )?;
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
        let has_filters = !matches!(team, EventTeam::All) || !matches!(target, EventTarget::All);
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
    ) -> Result<EventTeam, IntegrationError> {
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
            return Ok(EventTeam::All);
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
            "ALL" => Ok(EventTeam::All),
            "TEAM_1" => Ok(EventTeam::Team1),
            "TEAM_2" => Ok(EventTeam::Team2),
            _ => Err(self.unsupported(
                format!("catalog EventTeam member '{member}' is not supported by canonical WIR"),
                argument.span.or(annotation.span),
            )),
        }
    }

    fn lower_event_target(
        &self,
        annotations: &[hir::Annotation],
    ) -> Result<EventTarget, IntegrationError> {
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
            return Ok(EventTarget::All);
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
        let locale = Locale::new("en-US");
        let catalog_spelling = match (domain, spelling.as_str()) {
            ("Hero", "mccree") => "CASSIDY",
            ("Hero", "hammond") => "WRECKING_BALL",
            ("Hero", "soldier") => "SOLDIER_76",
            ("Hero", "domina") => "JINYU",
            ("Hero", "dmon") => "D_MON",
            _ => spelling.as_str(),
        };
        let member = self
            .compiler
            .catalog
            .resolve_enum_member(domain, &locale, catalog_spelling)
            .map(|(_, member)| member)
            .or_else(|| {
                (domain == "Hero")
                    .then(|| {
                        self.compiler
                            .catalog
                            .enum_domain(domain)
                            .and_then(|domain| {
                                domain
                                    .members
                                    .iter()
                                    .find(|member| {
                                        member.member.eq_ignore_ascii_case(catalog_spelling)
                                            || member.spellings(&locale).iter().any(|candidate| {
                                                candidate.eq_ignore_ascii_case(catalog_spelling)
                                            })
                                            || member
                                                .member
                                                .chars()
                                                .filter(|c| c.is_ascii_alphanumeric())
                                                .collect::<String>()
                                                .eq_ignore_ascii_case(
                                                    &catalog_spelling
                                                        .chars()
                                                        .filter(|c| c.is_ascii_alphanumeric())
                                                        .collect::<String>(),
                                                )
                                    })
                                    .map(|member| member.member.clone())
                            })
                    })
                    .flatten()
            })
            .ok_or_else(|| {
                self.unsupported(
                    format!("unknown {domain} filter '{spelling}'"),
                    argument.span.or(annotation.span),
                )
            })?;
        if domain == "EventPlayer" {
            if member == "ALL" {
                Ok(EventTarget::All)
            } else if let Some(slot) = member.strip_prefix("SLOT_") {
                let slot = slot.parse::<u8>().map_err(|_| {
                    self.unsupported(
                        format!("catalog EventPlayer member '{member}' is not a slot"),
                        argument.span.or(annotation.span),
                    )
                })?;
                Ok(EventTarget::Slot(slot))
            } else {
                Err(self.unsupported(
                    format!(
                        "catalog EventPlayer member '{member}' is not supported by canonical WIR"
                    ),
                    argument.span.or(annotation.span),
                ))
            }
        } else {
            Ok(EventTarget::Hero(member))
        }
    }

    fn lower_actions(
        &mut self,
        statements: &[Stmt],
        break_target: Option<BreakTarget>,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        self.visible_labels.push(
            statements
                .iter()
                .filter_map(|statement| match statement {
                    Stmt::Label { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect(),
        );
        let mut actions = Vec::new();
        let mut labels = HashMap::new();
        let mut gotos = Vec::new();
        let mut index = 0;
        while index < statements.len() {
            let statement = &statements[index];
            if let Stmt::If {
                branches,
                r#else,
                span,
            } = statement
            {
                if branches.len() == 1 && r#else.is_none() {
                    let branch = &branches[0];
                    if let Some((conditions, label)) =
                        branch.body.first().and_then(pure_goto_conditions)
                    {
                        let target = statements[index + 1..]
                            .iter()
                            .position(|candidate| {
                                matches!(candidate, Stmt::Label { name, .. } if name == label)
                            });
                        if target.is_some() && conditions.is_empty() {
                            let condition = self.lower_value(&branch.condition)?;
                            let condition = self.push_call("not", vec![condition]);
                            let body = self.lower_actions(&branch.body[1..], break_target)?;
                            actions.extend(self.push_if_actions(vec![(condition, body)], None));
                            index += 1;
                            continue;
                        }
                    }
                }
                let external_jump =
                    branches
                        .iter()
                        .enumerate()
                        .find_map(|(branch_index, branch)| {
                            let (conditions, label) = pure_goto_conditions(branch.body.last()?)?;
                            let target = statements[index + 1..]
                                .iter()
                                .position(|candidate| {
                                    matches!(candidate, Stmt::Label { name, .. } if name == label)
                                })
                                .map(|offset| index + 1 + offset)
                                .or_else(|| {
                                    self.outer_goto_targets
                                        .last()
                                        .is_some_and(|target| target == label)
                                        .then_some(statements.len())
                                })?;
                            Some((
                                branch_index,
                                conditions.into_iter().cloned().collect::<Vec<_>>(),
                                label.to_string(),
                                target,
                            ))
                        });
                if let Some((exit_branch, exit_conditions, label, target)) = external_jump {
                    self.outer_goto_targets.push(label.clone());
                    let middle_result =
                        self.lower_actions(&statements[index + 1..target], break_target);
                    let middle = middle_result?;
                    self.resolve_deferred_gotos(&middle, label.as_str(), middle.len())?;
                    let distance = self.canonical_action_width(&middle, *span)?;
                    let mut lowered_branches = Vec::with_capacity(branches.len());
                    for (branch_index, branch) in branches.iter().enumerate() {
                        let body = if branch_index == exit_branch {
                            &branch.body[..branch.body.len() - 1]
                        } else {
                            &branch.body[..]
                        };
                        lowered_branches.push((
                            self.lower_value(&branch.condition)?,
                            self.lower_actions(body, break_target)?,
                        ));
                    }
                    let else_body = r#else
                        .as_ref()
                        .map(|body| self.lower_actions(body, break_target))
                        .transpose()?;
                    self.outer_goto_targets.pop();
                    actions.extend(self.push_if_actions(lowered_branches, else_body));
                    let mut condition = self.lower_value(&branches[exit_branch].condition)?;
                    for expression in exit_conditions {
                        let right = self.lower_value(&expression)?;
                        condition = self.push_call("and", vec![condition, right]);
                    }
                    let distance_value = self.push_number(distance as f64, &distance.to_string());
                    actions.push(self.push_call_action("skipIf", &[condition, distance_value]));
                    actions.extend(middle);
                    index = target + 1;
                    continue;
                }
            }
            if let Some((conditions, label)) = pure_goto_conditions(statement) {
                let target = statements[index + 1..]
                    .iter()
                    .position(
                        |candidate| matches!(candidate, Stmt::Label { name, .. } if name == label),
                    )
                    .map(|offset| index + 1 + offset)
                    .or_else(|| {
                        self.outer_goto_targets
                            .last()
                            .is_some_and(|target| target == label)
                            .then_some(statements.len())
                    });
                if let Some(target) = target {
                    self.outer_goto_targets.push(label.to_string());
                    let middle_result =
                        self.lower_actions(&statements[index + 1..target], break_target);
                    self.outer_goto_targets.pop();
                    let middle = middle_result?;
                    let span = statement.span().copied();
                    self.resolve_deferred_gotos(&middle, label, middle.len())?;
                    let distance = self.canonical_action_width(&middle, span)?;
                    let mut args = Vec::with_capacity(conditions.len() + 1);
                    if let Some((first, rest)) = conditions.split_first() {
                        let mut condition = self.lower_value(first)?;
                        for expression in rest {
                            let right = self.lower_value(expression)?;
                            condition = self.push_call("and", vec![condition, right]);
                        }
                        args.push(condition);
                    }
                    args.push(self.push_number(distance as f64, &distance.to_string()));
                    actions.push(self.push_call_action(
                        if conditions.is_empty() {
                            "skip"
                        } else {
                            "skipIf"
                        },
                        &args,
                    ));
                    actions.extend(middle);
                    index = target + 1;
                    continue;
                }
            }
            match statement {
                Stmt::Label { name, .. } => {
                    labels.insert(name.clone(), actions.len());
                }
                Stmt::Goto {
                    label,
                    offset,
                    rule_start,
                    span,
                } => {
                    if *rule_start {
                        return Err(self.unsupported(
                            "goto RULE_START is not representable in canonical WIR",
                            *span,
                        ));
                    }
                    let placeholder = self.push_number(0.0, "0");
                    let action = self.push_call_action("skip", &[placeholder]);
                    let position = actions.len();
                    actions.push(action);
                    gotos.push((
                        action,
                        position,
                        label.clone(),
                        offset.clone(),
                        span.map(Into::into),
                    ));
                }
                _ => actions.extend(self.lower_action(statement, break_target)?),
            }
            index += 1;
        }
        for (action, position, label, offset, span) in gotos {
            let distance = if let Some(offset) = offset {
                self.lower_value(&offset)?
            } else {
                let Some(label) = label else {
                    return Err(self.unsupported("goto is missing a label or offset", span));
                };
                let Some(&target) = labels.get(&label) else {
                    if self
                        .visible_labels
                        .iter()
                        .any(|labels| labels.contains(&label))
                    {
                        self.deferred_gotos.push((action, label, span));
                        continue;
                    }
                    return Err(self.unsupported(format!("unknown goto label '{label}'"), span));
                };
                if target < position {
                    return Err(self
                        .unsupported("backward goto is not representable in canonical WIR", span));
                }
                let width = self.canonical_action_width(&actions[position + 1..target], span)?;
                self.push_number(width as f64, &width.to_string())
            };
            let distance = self.value(distance).clone();
            let Some(Action::Call { args, .. }) = self.actions.get_mut(action) else {
                unreachable!("goto placeholder must be a call action")
            };
            args[0] = distance;
        }
        if self.visible_labels.len() == 1 && !self.deferred_gotos.is_empty() {
            let (_, label, span) = self.deferred_gotos.remove(0);
            return Err(self.unsupported(format!("unknown goto label '{label}'"), span));
        }
        self.visible_labels.pop();
        Ok(actions)
    }

    fn resolve_deferred_gotos(
        &mut self,
        actions: &[ActionId],
        label: &str,
        target: usize,
    ) -> Result<(), IntegrationError> {
        let deferred = std::mem::take(&mut self.deferred_gotos);
        let mut remaining = Vec::new();
        for (action, deferred_label, span) in deferred {
            if deferred_label != label {
                remaining.push((action, deferred_label, span));
                continue;
            }
            let Some(position) = actions.iter().position(|candidate| *candidate == action) else {
                remaining.push((action, deferred_label, span));
                continue;
            };
            if target < position {
                return Err(
                    self.unsupported("backward goto is not representable in canonical WIR", span)
                );
            }
            let width = self.canonical_action_width(&actions[position + 1..target], span)?;
            let distance = self.push_number(width as f64, &width.to_string());
            let distance = self.value(distance).clone();
            let Some(Action::Call { args, .. }) = self.actions.get_mut(action) else {
                unreachable!("deferred goto placeholder must be a call action")
            };
            args[0] = distance;
        }
        self.deferred_gotos = remaining;
        Ok(())
    }

    fn lower_action(
        &mut self,
        stmt: &Stmt,
        break_target: Option<BreakTarget>,
    ) -> Result<Vec<ActionId>, IntegrationError> {
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
                span: _,
            } => {
                let branches = branches
                    .iter()
                    .map(|branch| {
                        Ok((
                            self.lower_value(&branch.condition)?,
                            self.lower_actions(&branch.body, break_target)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?;
                let else_body = r#else
                    .as_ref()
                    .map(|body| self.lower_actions(body, break_target))
                    .transpose()?;
                Ok(self.push_if_actions(branches, else_body))
            }
            Stmt::For {
                variable,
                iterable,
                body,
                span: _,
            } => {
                let (start, stop, step) = self.lower_range(iterable)?;
                let body = self.lower_loop_body(body)?;
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
                        Ok(self.push_for_global_actions(variable_id, start, stop, step, body))
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
                        Ok(self.push_for_player_actions(
                            player, variable_id, start, stop, step, body,
                        ))
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
                span: _,
            } => {
                let condition = self.lower_value(condition)?;
                let body = self.lower_loop_body(body)?;
                Ok(self.push_while_actions(condition, body))
            }
            Stmt::DoWhile {
                condition,
                body,
                span: _,
            } => {
                let body = self.lower_do_while_body(body)?;
                let condition = self.lower_value(condition)?;
                let loop_if = self.push_call_action("loopIf", &[condition]);
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
            } => self.lower_switch(value, arms, *span),
            Stmt::Delete { target, span } => self.lower_delete(target, *span).map(|action| vec![action]),
            Stmt::Continue { span } => Err(self.unsupported(
                "continue statements are only lowered while constructing a loop body",
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
                Some(BreakTarget::Loop) => Ok(vec![self.push_call_action("break", &[])]),
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
            Stmt::Return { span: _ } => {
                let true_value = self.push_value(Value::Bool(true));
                Ok(vec![self.push_call_action("abortIf", &[true_value])])
            }
            Stmt::Expr { expr, span } => match expr.as_ref() {
                Expr::Call { name, args, .. } => {
                    if name == "disableInspector" && args.is_empty() {
                        Ok(vec![self.push_call_action("disableInspector", &[])])
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
                Ok(vec![self.push_action(Action::CallSubroutine {
                    subroutine: self.subroutine_names[subroutine].clone(),
                })])
            }
        }
    }

    fn lower_loop_body(&mut self, statements: &[Stmt]) -> Result<Vec<ActionId>, IntegrationError> {
        self.lower_loop_sequence(statements, &[], 0)
    }

    fn lower_loop_sequence(
        &mut self,
        statements: &[Stmt],
        after: &[ActionId],
        structural_after: usize,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let mut actions = Vec::new();
        let mut index = 0;
        while index < statements.len() {
            let statement = &statements[index];
            let tail = &statements[index + 1..];
            if let Some(conditions) = pure_continue_conditions(statement) {
                let tail = self.lower_loop_sequence(tail, after, structural_after)?;
                let distance = self.canonical_action_width(&tail, statement.span().copied())?
                    + structural_after
                    + self.canonical_action_width(after, statement.span().copied())?;
                if distance > 0 {
                    let mut args = Vec::with_capacity(conditions.len() + 1);
                    if let Some((first, rest)) = conditions.split_first() {
                        let mut condition = self.lower_value(first)?;
                        for expression in rest {
                            let right = self.lower_value(expression)?;
                            condition = self.push_call("and", vec![condition, right]);
                        }
                        args.push(condition);
                    }
                    let distance = self.push_number(distance as f64, &distance.to_string());
                    args.push(distance);
                    actions.push(self.push_call_action(
                        if conditions.is_empty() {
                            "skip"
                        } else {
                            "skipIf"
                        },
                        &args,
                    ));
                }
                actions.extend(tail);
                return Ok(actions);
            }
            if contains_loop_continue(statement) {
                let tail = self.lower_loop_sequence(tail, after, structural_after)?;
                let mut continuation_after = tail.clone();
                continuation_after.extend_from_slice(after);
                let lowered = self.lower_if_with_loop_continue(
                    statement,
                    &continuation_after,
                    structural_after,
                )?;
                actions.extend(lowered);
                actions.extend(tail);
                return Ok(actions);
            }
            if let Some((conditions, label)) = pure_goto_conditions(statement) {
                if let Some(target) = statements[index + 1..]
                    .iter()
                    .position(
                        |candidate| matches!(candidate, Stmt::Label { name, .. } if name == label),
                    )
                    .map(|offset| index + 1 + offset)
                {
                    let middle = self.lower_loop_sequence(
                        &statements[index + 1..target],
                        after,
                        structural_after,
                    )?;
                    let suffix = self.lower_loop_sequence(
                        &statements[target + 1..],
                        after,
                        structural_after,
                    )?;
                    let distance =
                        self.canonical_action_width(&middle, statement.span().copied())?;
                    let mut args = Vec::with_capacity(conditions.len() + 1);
                    if let Some((first, rest)) = conditions.split_first() {
                        let mut condition = self.lower_value(first)?;
                        for expression in rest {
                            let right = self.lower_value(expression)?;
                            condition = self.push_call("and", vec![condition, right]);
                        }
                        args.push(condition);
                    }
                    args.push(self.push_number(distance as f64, &distance.to_string()));
                    actions.push(self.push_call_action(
                        if conditions.is_empty() {
                            "skip"
                        } else {
                            "skipIf"
                        },
                        &args,
                    ));
                    actions.extend(middle);
                    actions.extend(suffix);
                    return Ok(actions);
                }
            }
            if let Some((conditions, label)) = pure_goto_conditions(statement) {
                let local_label = statements.iter().any(
                    |candidate| matches!(candidate, Stmt::Label { name, .. } if name == label),
                );
                let outer_label = self
                    .visible_labels
                    .last()
                    .is_some_and(|labels| labels.contains(label));
                if !local_label && outer_label {
                    let _span = statement.span().copied();
                    let mut condition = None;
                    for expression in conditions {
                        let value = self.lower_value(expression)?;
                        condition = Some(match condition {
                            Some(left) => self.push_call("and", vec![left, value]),
                            None => value,
                        });
                    }
                    let break_action = self.push_call_action("break", &[]);
                    if let Some(condition) = condition {
                        actions.extend(
                            self.push_if_actions(vec![(condition, vec![break_action])], None),
                        );
                    } else {
                        actions.push(break_action);
                    }
                    index += 1;
                    continue;
                }
            }
            actions.extend(self.lower_action(statement, Some(BreakTarget::Loop))?);
            index += 1;
        }
        Ok(actions)
    }

    fn lower_if_with_loop_continue(
        &mut self,
        statement: &Stmt,
        after: &[ActionId],
        structural_after: usize,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let Stmt::If {
            branches,
            r#else,
            span: _,
        } = statement
        else {
            unreachable!("continue-containing loop statement must be an if")
        };
        let mut lowered_branches = Vec::with_capacity(branches.len());
        let mut suffix = after.to_vec();
        let mut suffix_structural = structural_after + 1;
        let mut lowered_else = None;
        if let Some(body) = r#else {
            let body = self.lower_loop_sequence(body, after, suffix_structural)?;
            suffix.splice(0..0, body.iter().copied());
            suffix_structural += 1;
            lowered_else = Some(body);
        }
        for index in (0..branches.len()).rev() {
            let body =
                self.lower_loop_sequence(&branches[index].body, &suffix, suffix_structural)?;
            suffix_structural += 1;
            suffix.splice(0..0, body.iter().copied());
            lowered_branches.push(body);
        }
        lowered_branches.reverse();
        let mut branch_actions = Vec::with_capacity(branches.len());
        for (branch, body) in branches.iter().zip(lowered_branches) {
            branch_actions.push((self.lower_value(&branch.condition)?, body));
        }
        Ok(self.push_if_actions(branch_actions, lowered_else))
    }

    fn lower_do_while_body(
        &mut self,
        statements: &[Stmt],
    ) -> Result<Vec<ActionId>, IntegrationError> {
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
                let (name, args, _span) = if let Stmt::Break { span } = statement {
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
                let distance = self.push_number(distance as f64, &distance.to_string());
                let mut args = args;
                args.push(distance);
                actions.push(self.push_call_action(name, &args));
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
    ) -> Result<(ValueId, ValueId, ValueId), IntegrationError> {
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
        let number = |this: &mut Self, value: f64| -> Result<ValueId, IntegrationError> {
            let _ = span;
            Ok(this.push_number(value, &value.to_string()))
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
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let selector = self.lower_value(value)?;
        let mut case_values = Vec::new();
        let mut lowered_arms = Vec::with_capacity(arms.len());
        let mut has_default = false;
        let mut legacy_case_offsets = Vec::new();
        let mut legacy_offset = 0;
        let mut legacy_default_offset = None;

        for arm in arms {
            let (value, (body, break_at)) = match arm {
                SwitchArm::Case { value, body, .. } => {
                    case_values.push(self.lower_value(value)?);
                    (Some(value), self.lower_switch_body(body)?)
                }
                SwitchArm::Default { body, span } => {
                    if has_default {
                        return Err(
                            self.unsupported("a switch may contain at most one default arm", *span)
                        );
                    }
                    has_default = true;
                    legacy_default_offset = Some(legacy_offset);
                    (None, self.lower_switch_body(body)?)
                }
            };
            if value.is_some() {
                legacy_case_offsets.push(legacy_offset);
            }
            legacy_offset +=
                self.canonical_action_width(&body, span)? + usize::from(break_at.is_some());
            lowered_arms.push((value.map(Box::as_ref), body, break_at));
        }

        let break_arms: Vec<_> = lowered_arms
            .iter()
            .enumerate()
            .filter_map(|(index, (_, _, break_at))| break_at.map(|break_at| (index, break_at)))
            .collect();
        let first_break = break_arms.first().copied();
        let has_later_reachable_actions =
            first_break.is_some_and(|(break_index, (break_at, _))| {
                lowered_arms[break_index].1.len() > break_at
                    || lowered_arms
                        .iter()
                        .skip(break_index + 1)
                        .any(|(_, body, _)| !body.is_empty())
            });
        let use_shared_exit = break_arms.len() > 1 && has_later_reachable_actions;

        let case_values = self.lower_array(case_values, span)?;
        let _ = span;
        if !use_shared_exit {
            let default_offset = legacy_default_offset.unwrap_or(legacy_offset);
            let offset_values = std::iter::once(default_offset)
                .chain(legacy_case_offsets)
                .map(|value| self.push_number(value as f64, &value.to_string()))
                .collect();
            let offsets = self.lower_array(offset_values, span)?;
            let skip = self.lower_switch_selector(selector, case_values, offsets, span)?;
            let true_value = self.push_value(Value::Bool(true));
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
            let result = self.push_if_actions(vec![(true_value, branch_body)], else_body);
            return Ok(result);
        }

        let offsets = self.push_value(Value::Array(Vec::new()));
        let skip = self.lower_switch_selector(selector, case_values, offsets, span)?;
        let mut arm_offsets = vec![None; lowered_arms.len()];
        let (switch, switch_end) =
            self.lower_switch_level(&lowered_arms, 0, Some(skip), 0, &mut arm_offsets, span)?;

        let default_offset = lowered_arms
            .iter()
            .enumerate()
            .find_map(|(index, (value, _, _))| value.is_none().then(|| arm_offsets[index].unwrap()))
            .unwrap_or(switch_end);
        let offset_values = std::iter::once(default_offset)
            .chain(
                lowered_arms
                    .iter()
                    .enumerate()
                    .filter(|(_, (value, _, _))| value.is_some())
                    .map(|(index, _)| arm_offsets[index].unwrap()),
            )
            .map(|value| self.push_number(value as f64, &value.to_string()))
            .collect();
        let offset_values = self.lower_array(offset_values, span)?;
        let offset_value = self.value(offset_values).clone();
        let Some(node) = self.values.get_mut(offsets) else {
            unreachable!("switch offset placeholder must exist")
        };
        *node = offset_value;

        let offset_value = self.value(offsets).clone();
        let Some(Action::Call { args, .. }) = self.actions.get_mut(skip) else {
            unreachable!("switch selector must be a call action")
        };
        let Some(Value::Call { args, .. }) = args.first_mut() else {
            unreachable!("switch selector condition must be a value call")
        };
        args[0] = offset_value;

        Ok(switch)
    }

    fn lower_switch_selector(
        &mut self,
        selector: ValueId,
        case_values: ValueId,
        offsets: ValueId,
        _span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let one = self.push_number(1.0, "1");
        let index = self.push_call("indexOfArrayValue", vec![case_values, selector]);
        let case_offset = self.push_call("add", vec![one, index]);
        let skip_condition = self.push_call("valueInArray", vec![offsets, case_offset]);
        Ok(self.push_call_action("skip", &[skip_condition]))
    }

    fn lower_switch_level(
        &mut self,
        arms: &[LoweredSwitchArm<'_>],
        start: usize,
        selector_skip: Option<ActionId>,
        level_offset: usize,
        arm_offsets: &mut [Option<usize>],
        span: Option<HirSpan>,
    ) -> Result<(Vec<ActionId>, usize), IntegrationError> {
        let break_index = (start..arms.len())
            .find(|index| arms[*index].2.is_some())
            .expect("switch level must contain a break");
        let mut branch_body = Vec::new();
        if let Some(selector_skip) = selector_skip {
            branch_body.push(selector_skip);
        }
        let mut branch_offset = 0;
        for index in start..=break_index {
            arm_offsets[index] = Some(if selector_skip.is_some() {
                level_offset + branch_offset
            } else if index == start {
                level_offset
            } else {
                level_offset + 1 + branch_offset
            });
            let (_, body, break_at) = &arms[index];
            let body = if index == break_index {
                &body[..break_at.as_ref().unwrap().0]
            } else {
                body.as_slice()
            };
            branch_offset += self.canonical_action_width(body, span)?;
            branch_body.extend(body.iter().copied());
        }
        let branch_width = self.canonical_action_width(&branch_body, span)?;
        let (_, break_body, Some((break_at, _))) = &arms[break_index] else {
            unreachable!("break index must point to a switch break")
        };
        let mut else_body = break_body[*break_at..].to_vec();
        let tail_width = self.canonical_action_width(&else_body, span)?;
        let else_content_start = if selector_skip.is_some() {
            level_offset + branch_width + tail_width
        } else {
            level_offset + branch_width + tail_width + 2
        };
        let has_next_break = (break_index + 1..arms.len()).any(|index| arms[index].2.is_some());
        let end_offset = if has_next_break {
            let (child, child_end) = self.lower_switch_level(
                arms,
                break_index + 1,
                None,
                else_content_start,
                arm_offsets,
                span,
            )?;
            else_body.extend(child);
            child_end
        } else {
            let mut offset = else_content_start;
            for index in break_index + 1..arms.len() {
                arm_offsets[index] = Some(offset);
                let (_, body, _) = &arms[index];
                offset += self.canonical_action_width(body, span)?;
                else_body.extend(body.iter().copied());
            }
            offset
        };
        let true_value = self.push_value(Value::Bool(true));
        let switch = self.push_if_actions(vec![(true_value, branch_body)], Some(else_body));
        Ok((switch, end_offset))
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

    fn canonical_action_width(
        &self,
        actions: &[ActionId],
        fallback_span: Option<HirSpan>,
    ) -> Result<usize, IntegrationError> {
        let public_actions = self.public_actions(actions);
        let mut program = self.program.clone();
        program.settings = None;
        program.rules.push(workshop_rs::Rule {
            name: "action layout".to_string(),
            disabled: false,
            event: workshop_rs::Event::Global,
            conditions: Vec::new(),
            actions: public_actions.clone(),
        });
        workshop_rs::emitter::action_width(
            &program,
            &self.compiler.catalog,
            &Locale::new("en-US"),
            &public_actions,
        )
        .map(|layout| layout.width)
        .map_err(|error| {
            let span = fallback_span;
            IntegrationError::new("workshop-action-layout", error.to_string(), span)
        })
    }

    fn lower_array(
        &mut self,
        elements: Vec<ValueId>,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let name = if elements.is_empty() {
            "emptyArray"
        } else {
            "array"
        };
        let elements = self.normalize_contextual_arguments(name, elements);
        let _ = span;
        Ok(self.push_value(Value::Call {
            name: name.to_string(),
            args: self.value_args(&elements),
        }))
    }

    fn lower_translation_helper(
        &mut self,
        translations: &hir::TranslationState,
    ) -> Result<ValueId, IntegrationError> {
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
    ) -> Result<ActionId, IntegrationError> {
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
        let sort_order = self.push_number(-9999.0, "-9999");
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
        let args = self.normalize_contextual_arguments(
            "createHudText",
            vec![
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
        );
        let _ = span;
        Ok(self.push_call_action("createHudText", &args))
    }

    fn lower_print(
        &mut self,
        expr: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        macro_rules! call {
            ($name:literal $(, $arg:expr)* $(,)?) => {{
                let args = vec![$($arg),*];
                self.push_call($name, args)
            }};
        }

        let message = self.lower_value(expr)?;
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
        let sort_order = self.push_number(-9999.0, "-9999");
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
        let args = self.normalize_contextual_arguments(
            "createHudText",
            vec![
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
        );
        let _ = span;
        Ok(self.push_call_action("createHudText", &args))
    }

    fn lower_debug_array_text(&mut self, value: ValueId) -> ValueId {
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

    fn lower_text_value(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        let value = self.lower_value(expr)?;
        let Value::Call { name, args } = self.value(value) else {
            return Ok(value);
        };
        if name == "customString" && args.len() == 1 {
            Ok(self.push_value(args[0].clone()))
        } else {
            Ok(value)
        }
    }

    fn debug_value_is_array(&self, value: ValueId) -> bool {
        match self.value(value) {
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

    fn value_is_known_player(&self, value: ValueId) -> bool {
        match self.value(value) {
            Value::EventPlayer => true,
            Value::Call { name, .. } => self
                .compiler
                .catalog
                .entry(Kind::Value, name)
                .and_then(|entry| entry.return_type())
                .is_some_and(|return_type| {
                    return_type.split('|').any(|part| part.trim() == "Player")
                }),
            _ => false,
        }
    }

    fn push_value(&mut self, value: Value) -> ValueId {
        let id = self.values.len();
        self.values.push(value);
        id
    }

    fn push_call(&mut self, name: &str, args: Vec<ValueId>) -> ValueId {
        let args = self.normalize_contextual_arguments(name, args);
        self.push_value(Value::Call {
            name: name.to_string(),
            args: self.value_args(&args),
        })
    }

    fn normalize_contextual_values(&mut self, call_id: &str, values: Vec<Value>) -> Vec<Value> {
        let ids = values
            .into_iter()
            .map(|value| self.push_value(value))
            .collect();
        let ids = self.normalize_contextual_arguments(call_id, ids);
        self.value_args(&ids)
    }

    fn contextual_coercions(&self, call_id: &str, arg_index: usize) -> Option<ParamCoercions> {
        [Kind::Action, Kind::Value].into_iter().find_map(|kind| {
            self.compiler
                .catalog
                .entry(kind, call_id)
                .and_then(|entry| entry.param_coercions(arg_index))
                .copied()
        })
    }

    fn normalize_value_with_coercions(
        &mut self,
        coercions: ParamCoercions,
        value_id: ValueId,
    ) -> ValueId {
        let Some(node) = self.values.get(value_id) else {
            return value_id;
        };
        let replacement = match node {
            Value::Bool(false) if coercions.false_as_number => Some(Value::Number(0.0)),
            Value::Bool(true) if coercions.true_as_number => Some(Value::Number(1.0)),
            Value::Number(value) if coercions.zero_as_null && *value == 0.0 => Some(Value::Null),
            Value::Vector { x, y, z }
                if coercions.null_vector_as_null
                    && matches!(x.as_ref(), Value::Number(value) if *value == 0.0)
                    && matches!(y.as_ref(), Value::Number(value) if *value == 0.0)
                    && matches!(z.as_ref(), Value::Number(value) if *value == 0.0) =>
            {
                Some(Value::Null)
            }
            Value::Call { name, args }
                if coercions.null_vector_as_null
                    && name == "vector"
                    && args.len() == 3
                    && args
                        .iter()
                        .all(|value| matches!(value, Value::Number(number) if *number == 0.0)) =>
            {
                Some(Value::Null)
            }
            Value::Call { name, args }
                if coercions.empty_array_as_string && name == "emptyArray" && args.is_empty() =>
            {
                Some(Value::String(String::new()))
            }
            Value::Call { name, args }
                if coercions.empty_array_as_string
                    && name == "customString"
                    && args.len() == 1
                    && matches!(args.first(), Some(Value::String(value)) if value.is_empty()) =>
            {
                Some(Value::String(String::new()))
            }
            Value::Array(elements) if coercions.empty_array_as_string && elements.is_empty() => {
                Some(Value::String(String::new()))
            }
            _ => None,
        };
        let Some(value) = replacement else {
            return value_id;
        };
        self.push_value(value)
    }

    fn normalize_contextual_argument(
        &mut self,
        call_id: &str,
        arg_index: usize,
        value_id: ValueId,
    ) -> ValueId {
        let Some(coercions) = self.contextual_coercions(call_id, arg_index) else {
            return value_id;
        };
        self.normalize_value_with_coercions(coercions, value_id)
    }

    fn normalize_modify_value(&mut self, op: ModifyOp, value_id: ValueId) -> ValueId {
        let coercions = match op {
            ModifyOp::Add
            | ModifyOp::Subtract
            | ModifyOp::Modulo
            | ModifyOp::Min
            | ModifyOp::Max
            | ModifyOp::RemoveFromArrayByIndex => ParamCoercions {
                false_as_number: true,
                true_as_number: true,
                ..Default::default()
            },
            ModifyOp::AppendToArray | ModifyOp::RemoveFromArray => ParamCoercions {
                zero_as_null: true,
                ..Default::default()
            },
            ModifyOp::Multiply | ModifyOp::Divide | ModifyOp::RaiseToPower => {
                return value_id;
            }
        };
        self.normalize_value_with_coercions(coercions, value_id)
    }

    fn modify_op_from_value(&self, value_id: ValueId) -> Option<ModifyOp> {
        let Value::Call { name, args } = self.values.get(value_id)? else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        match name.as_str() {
            "add" => Some(ModifyOp::Add),
            "subtract" => Some(ModifyOp::Subtract),
            "multiply" => Some(ModifyOp::Multiply),
            "divide" => Some(ModifyOp::Divide),
            "modulo" => Some(ModifyOp::Modulo),
            "min" => Some(ModifyOp::Min),
            "max" => Some(ModifyOp::Max),
            "raiseToPower" => Some(ModifyOp::RaiseToPower),
            "appendToArray" => Some(ModifyOp::AppendToArray),
            "removeFromArray" | "removeFromArrayByValue" => Some(ModifyOp::RemoveFromArray),
            "removeFromArrayByIndex" => Some(ModifyOp::RemoveFromArrayByIndex),
            _ => None,
        }
    }

    fn normalize_contextual_arguments(
        &mut self,
        call_id: &str,
        mut args: Vec<ValueId>,
    ) -> Vec<ValueId> {
        let mut index = 0;
        while index < args.len() {
            args[index] = self.normalize_contextual_argument(call_id, index, args[index]);
            if matches!(
                call_id,
                "modifyGlobalVariableAtIndex" | "modifyPlayerVariableAtIndex"
            ) && index == 3
            {
                if let Some(op) = args
                    .get(2)
                    .and_then(|value_id| self.modify_op_from_value(*value_id))
                {
                    args[index] = self.normalize_modify_value(op, args[index]);
                }
            }
            index += 1;
        }
        args
    }

    fn lower_custom_string(
        &mut self,
        value: String,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let _ = span;
        let text = self.push_value(Value::String(value));
        Ok(self.push_call("customString", vec![text]))
    }

    fn push_number(&mut self, value: f64, text: &str) -> ValueId {
        let _ = text;
        self.push_value(Value::Number(value))
    }

    fn canonical_vector_member(&self, x: ValueId, y: ValueId, z: ValueId) -> Option<&'static str> {
        let number = |id| match self.values.get(id)? {
            Value::Number(value) => Some(*value),
            _ => None,
        };
        match (number(x), number(y), number(z)) {
            (Some(0.0), Some(1.0), Some(0.0)) => Some("UP"),
            _ => None,
        }
    }

    fn fold_numeric_binary(&self, op: &str, left: ValueId, right: ValueId) -> Option<f64> {
        let number = |id| match self.values.get(id)? {
            Value::Number(value) => Some(*value),
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

    fn lower_condition(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        if let Expr::Unary { op, operand, .. } = expr
            && op == "not"
            && !matches!(operand.as_ref(), Expr::Binary { .. })
        {
            let value = self.lower_value(operand)?;
            let false_value = self.push_value(Value::Bool(false));
            return Ok(self.push_call("==", vec![value, false_value]));
        }
        let value = self.lower_value(expr)?;
        let is_comparison = |expr: &Expr| matches!(expr, Expr::Binary { op, .. } if matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">="));
        if is_comparison(expr)
            || matches!(expr, Expr::Unary { op, operand, .. } if op == "not" && is_comparison(operand))
        {
            return Ok(value);
        }
        let true_value = self.push_value(Value::Bool(true));
        Ok(self.push_call("==", vec![value, true_value]))
    }

    fn lower_delete(
        &mut self,
        target: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let Expr::Index { array, index, .. } = target else {
            return Err(self.unsupported(
                "delete statements require an indexed global or player variable",
                span,
            ));
        };
        let (root, assignment) = match array.as_ref() {
            Expr::GlobalVar {
                name,
                span: target_span,
            } => {
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                let value =
                    self.push_value(Value::GlobalVariable(self.global_names[variable].clone()));
                (value, DeleteAssignment::Global(variable))
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
                let value = self.push_value(Value::PlayerVariable {
                    player: Box::new(self.value(player).clone()),
                    variable: self.player_names[variable].clone(),
                });
                (value, DeleteAssignment::Player { player, variable })
            }
            _ => {
                return Err(self.unsupported(
                    "delete statements are only representable for global or player variables",
                    target.span().copied(),
                ));
            }
        };
        let index = self.lower_value(index)?;
        let zero = self.push_number(0.0, "0");
        let one = self.push_number(1.0, "1");
        let end = self.push_call("add", vec![index, one]);
        let maximum = self.push_number(999_999_999_999.0, "999999999999");
        let prefix = self.push_call("slice", vec![root, zero, index]);
        let suffix = self.push_call("slice", vec![root, end, maximum]);
        let value = self.push_call("appendToArray", vec![prefix, suffix]);
        let _ = span;
        Ok(match assignment {
            DeleteAssignment::Global(variable) => self.push_action(Action::SetGlobalVariable {
                variable: self.global_names[variable].clone(),
                value: self.value(value).clone(),
            }),
            DeleteAssignment::Player { player, variable } => {
                self.push_action(Action::SetPlayerVariable {
                    player: self.value(player).clone(),
                    variable: self.player_names[variable].clone(),
                    value: self.value(value).clone(),
                })
            }
        })
    }

    fn lower_assign(
        &mut self,
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
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
                                let right = self.lower_value(right)?;
                                let val = self.normalize_modify_value(modify_op, right);
                                return Ok(self.push_action(Action::ModifyGlobalVariable {
                                    variable: self.global_names[variable].clone(),
                                    op: modify_op,
                                    value: self.value(val).clone(),
                                }));
                            }
                        }
                    }
                }
                let val = self.lower_value(value)?;
                Ok(self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value: self.value(val).clone(),
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
                                let right = self.lower_value(right)?;
                                let val = self.normalize_modify_value(modify_op, right);
                                return Ok(self.push_action(Action::ModifyPlayerVariable {
                                    player: self.value(player_val).clone(),
                                    variable: self.player_names[variable].clone(),
                                    op: modify_op,
                                    value: self.value(val).clone(),
                                }));
                            }
                        }
                    }
                }
                let val = self.lower_value(value)?;
                Ok(self.push_action(Action::SetPlayerVariable {
                    player: self.value(player_val).clone(),
                    variable: self.player_names[variable].clone(),
                    value: self.value(val).clone(),
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
                    let var_node = self.push_value(Value::GlobalVariable(
                        self.global_names[variable].clone(),
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
                                if let Some(modify_op) = modify_op_from_str(op) {
                                    let op_id = modify_catalog_name_from_str(op)
                                        .expect("known modify operator has a catalog name");
                                    let op_node = self.push_call(op_id, Vec::new());
                                    let right = self.lower_value(right)?;
                                    let right_val = self.normalize_modify_value(modify_op, right);
                                    let args = self.normalize_contextual_arguments(
                                        "modifyGlobalVariableAtIndex",
                                        vec![var_node, index_val, op_node, right_val],
                                    );
                                    return Ok(self.push_call_action(
                                        "modifyGlobalVariableAtIndex",
                                        &args,
                                    ));
                                }
                            }
                        }
                    }
                    let val = self.lower_value(value)?;
                    let args = self.normalize_contextual_arguments(
                        "setGlobalVariableAtIndex",
                        vec![var_node, index_val, val],
                    );
                    Ok(self.push_call_action("setGlobalVariableAtIndex", &args))
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
                    let var_node = self.push_value(Value::PlayerVariable {
                        player: Box::new(self.value(player_val).clone()),
                        variable: self.player_names[variable].clone(),
                    });
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
                                if let Some(modify_op) = modify_op_from_str(op) {
                                    let op_id = modify_catalog_name_from_str(op)
                                        .expect("known modify operator has a catalog name");
                                    let op_node = self.push_call(op_id, Vec::new());
                                    let right = self.lower_value(right)?;
                                    let right_val = self.normalize_modify_value(modify_op, right);
                                    let args = self.normalize_contextual_arguments(
                                        "modifyPlayerVariableAtIndex",
                                        vec![var_node, index_val, op_node, right_val],
                                    );
                                    // The canonical signature takes the
                                    // player-variable value node (which
                                    // carries the player) as its first
                                    // argument.
                                    return Ok(self.push_call_action(
                                        "modifyPlayerVariableAtIndex",
                                        &args,
                                    ));
                                }
                            }
                        }
                    }
                    let val = self.lower_value(value)?;
                    let args = self.normalize_contextual_arguments(
                        "setPlayerVariableAtIndex",
                        vec![var_node, index_val, val],
                    );
                    // The canonical signature takes the player-variable
                    // value node (which carries the player) as its first
                    // argument.
                    Ok(self.push_call_action("setPlayerVariableAtIndex", &args))
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
    ) -> Result<ActionId, IntegrationError> {
        let (action_name, root_value) = match root {
            Expr::GlobalVar {
                name,
                span: target_span,
            } => {
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                let root_value =
                    self.push_value(Value::GlobalVariable(self.global_names[variable].clone()));
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
                let root_value = self.push_value(Value::PlayerVariable {
                    player: Box::new(self.value(player_value).clone()),
                    variable: self.player_names[variable].clone(),
                });
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
        let args = self.normalize_contextual_arguments(
            action_name,
            vec![root_value, outer_index, replacement],
        );
        Ok(self.push_call_action(action_name, &args))
    }

    fn rebuild_indexed_value(
        &mut self,
        array: ValueId,
        indices: &[&Expr],
        target: &Expr,
        value: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
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
        array: ValueId,
        index: &Expr,
        index_value: ValueId,
    ) -> Result<ValueId, IntegrationError> {
        if matches!(index, Expr::Number { value, .. } if *value == 0.0) {
            Ok(self.push_call("firstOf", vec![array]))
        } else {
            Ok(self.push_call("valueInArray", vec![array, index_value]))
        }
    }

    fn replace_array_element(
        &mut self,
        array: ValueId,
        index: ValueId,
        replacement: ValueId,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
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
    ) -> Result<ActionId, IntegrationError> {
        if args.is_empty() {
            if let Some(&subroutine) = self.subroutines.get(name) {
                return Ok(self.push_action(Action::CallSubroutine {
                    subroutine: self.subroutine_names[subroutine].clone(),
                }));
            }
        }
        if name == "chaseAtRate" {
            let args = args
                .iter()
                .map(|expr| self.lower_value(expr))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(self.push_call_action(name, &args));
        }
        let function = self
            .compiler
            .manifest
            .resolve_function(name)
            .ok_or_else(|| self.unsupported(format!("unknown action '{name}'"), span))?;
        if !matches!(function.kind, FunctionKind::Action) {
            return Err(self.unsupported(format!("'{name}' is not a generic OPY action"), span));
        }
        if function.id == "async" {
            let [subroutine, behavior] = args else {
                return Err(
                    self.unsupported("async requires a subroutine and an AsyncBehavior", span)
                );
            };
            let subroutine_name = match subroutine {
                Expr::Call { name, args, .. } if args.is_empty() => name,
                _ => {
                    return Err(self.unsupported(
                        "async requires a declared subroutine",
                        subroutine.span().copied(),
                    ));
                }
            };
            let subroutine_id = *self.subroutines.get(subroutine_name).ok_or_else(|| {
                self.unsupported(
                    format!("unknown subroutine '{subroutine_name}'"),
                    subroutine.span().copied(),
                )
            })?;
            let subroutine = self.push_value(Value::Subroutine(
                self.subroutine_names[subroutine_id].clone(),
            ));
            let behavior = self.lower_value(behavior)?;
            return Ok(self.push_call_action("startRule", &[subroutine, behavior]));
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
                zero_vector.push(self.push_number(value, "0"));
            }
            lowered.push(self.push_call("vector", zero_vector));
            let args = self.normalize_contextual_arguments("createDummyBot", lowered);
            return Ok(self.push_call_action("createDummyBot", &args));
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
        let args = self.normalize_catalog_argument_domains(catalog_id, args);
        Ok(self.push_call_action(catalog_id.clone(), &args))
    }

    fn normalize_catalog_argument_domains(
        &mut self,
        catalog_id: &str,
        mut args: Vec<ValueId>,
    ) -> Vec<ValueId> {
        if self
            .compiler
            .catalog
            .entry(Kind::Action, catalog_id)
            .is_none()
        {
            return args;
        }
        let mut index = 0;
        while index < args.len() {
            args[index] = self.normalize_contextual_argument(catalog_id, index, args[index]);
            let Some(domain) = self
                .compiler
                .catalog
                .entry(Kind::Action, catalog_id)
                .and_then(|entry| entry.param_domain(index))
                .map(str::to_string)
            else {
                index += 1;
                continue;
            };
            let Some(Value::Enum { value_type, value }) = self.values.get(args[index]) else {
                index += 1;
                continue;
            };
            if value_type == "Team" && domain == "Color" {
                args[index] = self.push_value(Value::Enum {
                    value_type: domain,
                    value: value.clone(),
                });
            }
            index += 1;
        }
        args
    }

    fn lower_hud_text(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
        text_slot: usize,
        function_name: &str,
    ) -> Result<ActionId, IntegrationError> {
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
        let args = self.normalize_contextual_arguments("createHudText", args);
        Ok(self.push_call_action("createHudText", &args))
    }

    fn lower_hud_visible_to(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        if let Expr::Call { name, args, .. } = expr {
            if name == "getAllPlayers" && args.is_empty() {
                return Ok(self.lower_all_players());
            }
        }
        self.lower_value(expr)
    }

    fn lower_all_players(&mut self) -> ValueId {
        let all_teams = self.push_value(Value::Enum {
            value_type: "Team".to_string(),
            value: "ALL".to_string(),
        });
        self.push_call("allPlayers", vec![all_teams])
    }

    fn lower_receiver_action_call(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
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
        if matches!(function.id.as_str(), "append" | "remove") {
            let [value] = args else {
                return Err(self.unsupported(
                    format!("{} requires exactly one argument", function.id),
                    span,
                ));
            };
            let op = if function.id == "append" {
                ModifyOp::AppendToArray
            } else {
                ModifyOp::RemoveFromArray
            };
            let value = self.lower_value(value)?;
            let value = self.normalize_modify_value(op, value);
            return match receiver {
                Expr::GlobalVar {
                    name,
                    span: target_span,
                } => {
                    let variable = *self.globals.get(name).ok_or_else(|| {
                        self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                    })?;
                    Ok(self.push_action(Action::ModifyGlobalVariable {
                        variable: self.global_names[variable].clone(),
                        op,
                        value: self.value(value).clone(),
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
                    Ok(self.push_action(Action::ModifyPlayerVariable {
                        player: self.value(player).clone(),
                        variable: self.player_names[variable].clone(),
                        op,
                        value: self.value(value).clone(),
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
        let args = self.normalize_contextual_arguments(catalog_id, lowered);
        Ok(self.push_call_action(catalog_id.clone(), &args))
    }

    fn lower_value(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        let span = expr.span().copied();
        if matches!(expr, Expr::Binary { .. } | Expr::Unary { .. })
            || matches!(expr, Expr::Call { name, .. } if matches!(name.as_str(), "len" | "countOf"))
        {
            let bindings = HashMap::new();
            let mut stack = Vec::new();
            if let Some(value) =
                crate::compile_time::evaluate(expr, &self.constants, &bindings, &mut stack)
            {
                match value {
                    crate::compile_time::Value::Number(value) if value.is_finite() => {
                        return Ok(self.push_number(value, &computed_number_text(value)));
                    }
                    crate::compile_time::Value::String(value) => {
                        return self.lower_custom_string(value, span);
                    }
                    crate::compile_time::Value::Bool(value) => {
                        return Ok(self.push_value(Value::Bool(value)));
                    }
                    crate::compile_time::Value::Array(_)
                    | crate::compile_time::Value::Object(_) => {}
                    crate::compile_time::Value::Number(_) => {}
                }
            }
        }
        let value = match expr {
            Expr::Number { value, .. } => Value::Number(*value),
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
                Value::GlobalVariable(self.global_names[id].clone())
            }
            Expr::PlayerVar { player, name, .. } => {
                let player = self.lower_value(player)?;
                let id = *self.players.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown player variable '{name}'"), span)
                })?;
                Value::PlayerVariable {
                    player: Box::new(self.value(player).clone()),
                    variable: self.player_names[id].clone(),
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
            Expr::Vector { x, y, z, .. } => {
                let x = self.lower_value(x)?;
                let y = self.lower_value(y)?;
                let z = self.lower_value(z)?;
                if let Some(member) = self.canonical_vector_member(x, y, z) {
                    Value::Enum {
                        value_type: "Vector".to_string(),
                        value: member.to_string(),
                    }
                } else {
                    Value::Call {
                        name: "vector".to_string(),
                        args: self.value_args(&[x, y, z]),
                    }
                }
            }
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
                    return Ok(self.push_value(Value::Null));
                }
                // The pinned OverPy oracle lowers a literal zero-index read
                // (`arr[0]`, `arr[0.0]`) to `firstOf(arr)`; non-zero indexes
                // and indexed writes keep the indexed forms.
                if matches!(index.as_ref(), Expr::Number { value, .. } if *value == 0.0) {
                    let array = self.lower_value(array)?;
                    Value::Call {
                        name: "firstOf".to_string(),
                        args: self.value_args(&[array]),
                    }
                } else {
                    let array = self.lower_value(array)?;
                    let index = self.lower_value(index)?;
                    Value::Call {
                        name: "valueInArray".to_string(),
                        args: self.value_args(&[array, index]),
                    }
                }
            }
            Expr::Format { text, args, .. } => {
                if let Some(value) = fold_literal_format(text, args) {
                    return self.lower_custom_string(value, span);
                }
                let format_text = canonical_format_text(text);
                if args.len() <= 3 {
                    let text_node = self.push_value(Value::String(format_text));
                    let mut call_args = vec![self.value(text_node).clone()];
                    for arg in args {
                        let arg = self.lower_value(arg)?;
                        call_args.push(self.value(arg).clone());
                    }
                    Value::Call {
                        name: "customString".to_string(),
                        args: call_args,
                    }
                } else {
                    let chunks = split_format_chunks(&format_text, args.len()).ok_or_else(|| {
                        self.unsupported(
                            "format strings with more than three replacements require sequential placeholders",
                            span,
                        )
                    })?;
                    let lowered_args = args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut parts = Vec::with_capacity(chunks.len());
                    for (chunk, indices) in chunks {
                        let text = self.push_value(Value::String(chunk));
                        let mut call_args = vec![self.value(text).clone()];
                        call_args.extend(
                            indices
                                .into_iter()
                                .map(|index| self.value(lowered_args[index]).clone()),
                        );
                        let call_args = self.normalize_contextual_values("customString", call_args);
                        parts.push(self.push_value(Value::Call {
                            name: "customString".to_string(),
                            args: call_args,
                        }));
                    }
                    let separator = self.push_value(Value::String("{0}{1}".to_string()));
                    let mut value = parts[0];
                    for part in parts.into_iter().skip(1) {
                        value = self.push_call("customString", vec![separator, value, part]);
                    }
                    return Ok(value);
                }
            }
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                ..
            } => Value::Call {
                name: "ifThenElse".to_string(),
                args: {
                    let condition = self.lower_value(condition)?;
                    let then_value = self.lower_value(then_value)?;
                    let else_value = self.lower_value(else_value)?;
                    self.value_args(&[condition, then_value, else_value])
                },
            },
            Expr::Binary {
                op, left, right, ..
            } => {
                let left = self.lower_value(left)?;
                let right = self.lower_value(right)?;
                if let Some(value) = self.fold_numeric_binary(op, left, right) {
                    Value::Number(value)
                } else {
                    match op.as_str() {
                        "==" | "!=" | "<" | "<=" | ">" | ">=" => Value::Call {
                            name: op.clone(),
                            args: self.value_args(&[left, right]),
                        },
                        "+" => Value::Call {
                            name: "add".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "-" => Value::Call {
                            name: "subtract".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "*" => Value::Call {
                            name: "multiply".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "/" => Value::Call {
                            name: "divide".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "%" => Value::Call {
                            name: "modulo".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "**" => Value::Call {
                            name: "raiseToPower".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "and" => Value::Call {
                            name: "and".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "or" => Value::Call {
                            name: "or".to_string(),
                            args: self.value_args(&[left, right]),
                        },
                        "in" => Value::Call {
                            name: "arrayContains".to_string(),
                            args: self.value_args(&[right, left]),
                        },
                        "not in" => {
                            let contains = self.push_call("arrayContains", vec![right, left]);
                            Value::Call {
                                name: "not".to_string(),
                                args: self.value_args(&[contains]),
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
                            let left = self.lower_value(left)?;
                            let right = self.lower_value(right)?;
                            Value::Call {
                                name: negated.to_string(),
                                args: self.value_args(&[left, right]),
                            }
                        } else {
                            let operand = self.lower_value(operand)?;
                            Value::Call {
                                name: "not".to_string(),
                                args: self.value_args(&[operand]),
                            }
                        }
                    } else {
                        let operand = self.lower_value(operand)?;
                        Value::Call {
                            name: "not".to_string(),
                            args: self.value_args(&[operand]),
                        }
                    }
                }
                "-" => Value::Call {
                    name: "-".to_string(),
                    args: {
                        let operand = self.lower_value(operand)?;
                        self.value_args(&[operand])
                    },
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
                if name == "localPlayer" && args.is_empty() {
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
                    let x = self.lower_value(&args[0])?;
                    let y = self.lower_value(&args[1])?;
                    let z = self.lower_value(&args[2])?;
                    if let Some(member) = self.canonical_vector_member(x, y, z) {
                        Value::Enum {
                            value_type: "Vector".to_string(),
                            value: member.to_string(),
                        }
                    } else {
                        Value::Vector {
                            x: Box::new(self.value(x).clone()),
                            y: Box::new(self.value(y).clone()),
                            z: Box::new(self.value(z).clone()),
                        }
                    }
                } else if matches!(
                    name.as_str(),
                    "createWorkshopSettingBool"
                        | "createWorkshopSettingEnum"
                        | "createWorkshopSettingInt"
                        | "createWorkshopSettingFloat"
                ) {
                    let mut lowered = args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    if name == "createWorkshopSettingFloat" && lowered.len() == 5 {
                        lowered.push(self.push_number(0.0, "0"));
                    }
                    Value::Call {
                        name: match name.as_str() {
                            "createWorkshopSettingBool" => "workshopSettingToggle",
                            "createWorkshopSettingEnum" => "workshopSettingCombo",
                            "createWorkshopSettingInt" => "workshopSettingInteger",
                            _ => name,
                        }
                        .to_string(),
                        args: self.value_args(&lowered),
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
                        args: self.value_args(&[array, condition]),
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
                    let value = self.lower_value(value)?;
                    Value::Call {
                        name: "roundToInteger".to_string(),
                        args: self.value_args(&[value, rounding]),
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
                        args: self.value_args(&[array, key]),
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
                    if function.id == "getAllPlayers" {
                        return Ok(self.lower_all_players());
                    }
                    let lowered_args = args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    Value::Call {
                        name: catalog_id.clone(),
                        args: self.value_args(&lowered_args),
                    }
                }
            }
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => {
                if matches!(name.as_str(), "all" | "any") {
                    let [
                        Expr::Lambda {
                            params, body, span, ..
                        },
                    ] = args.as_slice()
                    else {
                        return Err(
                            self.unsupported(format!("{name} requires one lambda argument"), span)
                        );
                    };
                    let condition = self.lower_array_callback(params, body, *span)?;
                    let receiver = self.lower_value(receiver)?;
                    let args = self.value_args(&[receiver, condition]);
                    return Ok(self.push_value(Value::Call {
                        name: if name == "all" {
                            "isTrueForAll"
                        } else {
                            "isTrueForAny"
                        }
                        .to_string(),
                        args,
                    }));
                }
                let function = self.compiler.manifest.resolve_member(name).ok_or_else(|| {
                    self.unsupported(format!("unknown member value '{name}'"), span)
                })?;
                if !matches!(function.kind, FunctionKind::MemberValue) {
                    return Err(self.unsupported(format!("'{name}' is not a member value"), span));
                }
                if matches!(
                    function.id.as_str(),
                    "getHitPosition" | "getPlayerHit" | "getNormal"
                ) {
                    let member_name = function.id.as_str();
                    let Expr::Call {
                        name: receiver_name,
                        args: receiver_args,
                        ..
                    } = receiver.as_ref()
                    else {
                        return Err(self.unsupported(
                            format!("{member_name} requires a raycast receiver"),
                            span,
                        ));
                    };
                    if receiver_name != "raycast" || !args.is_empty() {
                        return Err(self.unsupported(
                            format!("{member_name} requires raycast(...) with no member arguments"),
                            span,
                        ));
                    }
                    let catalog_id = function.catalog_id.clone().ok_or_else(|| {
                        self.unsupported(
                            format!("{member_name} has no canonical catalog identity"),
                            span,
                        )
                    })?;
                    let lowered_args = receiver_args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(self.push_value(Value::Call {
                        name: catalog_id,
                        args: self.value_args(&lowered_args),
                    }));
                }
                if function.id == "filter" {
                    let [
                        Expr::Lambda {
                            params, body, span, ..
                        },
                    ] = args.as_slice()
                    else {
                        return Err(self.unsupported("filter requires one lambda argument", span));
                    };
                    let condition = self.lower_array_callback(params, body, *span)?;
                    let receiver = self.lower_value(receiver)?;
                    Value::Call {
                        name: "filteredArray".to_string(),
                        args: self.value_args(&[receiver, condition]),
                    }
                } else if matches!(function.id.as_str(), "concat" | "exclude") {
                    let [value] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{} requires exactly one argument", function.id),
                            span,
                        ));
                    };
                    let receiver = self.lower_value(receiver)?;
                    let value = self.lower_value(value)?;
                    Value::Call {
                        name: if function.id == "concat" {
                            "appendToArray"
                        } else {
                            "removeFromArray"
                        }
                        .to_string(),
                        args: self.value_args(&[receiver, value]),
                    }
                } else {
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
                        args: self.value_args(&lowered),
                    }
                }
            }
            Expr::Member {
                receiver, member, ..
            } => {
                let receiver = self.lower_value(receiver)?;
                if let Some(name) = match member.as_str() {
                    "x" => Some("__xComponentOf__"),
                    "y" => Some("__yComponentOf__"),
                    "z" => Some("__zComponentOf__"),
                    _ => None,
                } {
                    Value::Call {
                        name: name.to_string(),
                        args: self.value_args(&[receiver]),
                    }
                } else {
                    let member = self.push_value(Value::String(member.clone()));
                    Value::Call {
                        name: "memberAccess".to_string(),
                        args: self.value_args(&[receiver, member]),
                    }
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
                let iterable = if self.value_is_known_player(iterable) {
                    self.push_call("array", vec![iterable])
                } else {
                    iterable
                };
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
                    args: self.value_args(&[iterable, element]),
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
        let value_id = self.push_value(value);
        let Some(Value::Call { name, args }) = self.values.get(value_id) else {
            return Ok(value_id);
        };
        let name = name.clone();
        let args = args.clone();
        let args = self.normalize_contextual_values(&name, args);
        if let Some(Value::Call {
            args: target_args, ..
        }) = self.values.get_mut(value_id)
        {
            *target_args = args;
        }
        Ok(value_id)
    }

    fn lower_array_callback(
        &mut self,
        params: &[String],
        body: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
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
    ) -> Result<ValueId, IntegrationError> {
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

    fn push_action(&mut self, action: Action) -> ActionId {
        let id = self.actions.len();
        self.actions.push(action);
        id
    }

    fn value_args(&self, ids: &[ValueId]) -> Vec<Value> {
        ids.iter().map(|id| self.value(*id).clone()).collect()
    }

    fn push_call_action(&mut self, name: impl Into<String>, args: &[ValueId]) -> ActionId {
        self.push_action(Action::Call {
            name: name.into(),
            args: self.value_args(args),
        })
    }

    fn push_if_actions(
        &mut self,
        branches: Vec<(ValueId, Vec<ActionId>)>,
        else_body: Option<Vec<ActionId>>,
    ) -> Vec<ActionId> {
        let mut result = Vec::new();
        for (index, (condition, body)) in branches.into_iter().enumerate() {
            result.push(self.push_action(if index == 0 {
                Action::If {
                    condition: self.value(condition).clone(),
                }
            } else {
                Action::ElseIf {
                    condition: self.value(condition).clone(),
                }
            }));
            result.extend(body);
        }
        if let Some(body) = else_body {
            result.push(self.push_action(Action::Else));
            result.extend(body);
        }
        result.push(self.push_action(Action::End));
        result
    }

    fn push_while_actions(&mut self, condition: ValueId, body: Vec<ActionId>) -> Vec<ActionId> {
        let mut result = vec![self.push_action(Action::While {
            condition: self.value(condition).clone(),
        })];
        result.extend(body);
        result.push(self.push_action(Action::End));
        result
    }

    fn push_for_global_actions(
        &mut self,
        variable: GlobalVarId,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
        body: Vec<ActionId>,
    ) -> Vec<ActionId> {
        let mut result = vec![self.push_action(Action::ForGlobalVariable {
            variable: self.global_names[variable].clone(),
            start: self.value(start).clone(),
            stop: self.value(stop).clone(),
            step: self.value(step).clone(),
        })];
        result.extend(body);
        result.push(self.push_action(Action::End));
        result
    }

    fn push_for_player_actions(
        &mut self,
        player: ValueId,
        variable: PlayerVarId,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
        body: Vec<ActionId>,
    ) -> Vec<ActionId> {
        let mut result = vec![self.push_action(Action::ForPlayerVariable {
            player: self.value(player).clone(),
            variable: self.player_names[variable].clone(),
            start: self.value(start).clone(),
            stop: self.value(stop).clone(),
            step: self.value(step).clone(),
        })];
        result.extend(body);
        result.push(self.push_action(Action::End));
        result
    }

    fn value(&self, id: ValueId) -> &Value {
        self.values.get(id).expect("lowered value id must resolve")
    }

    pub(super) fn workshop_span(
        &self,
        span: Option<HirSpan>,
    ) -> Result<Option<workshop_rs::source::Span>, IntegrationError> {
        let Some(span) = span else {
            return Ok(None);
        };
        if !self.hir.files.iter().any(|file| file.id == span.file) {
            return Err(IntegrationError::new(
                "source-file",
                format!("HIR span references unknown source file id {}", span.file),
                Some(span),
            ));
        }
        Ok(Some(workshop_rs::source::Span::new(
            workshop_rs::source::FileId::from_index(span.file as usize),
            workshop_rs::source::Position::new(span.start.line, span.start.col),
            workshop_rs::source::Position::new(span.end.line, span.end.col),
        )))
    }

    fn public_actions(&self, actions: &[ActionId]) -> Vec<Action> {
        actions.iter().map(|id| self.actions[*id].clone()).collect()
    }

    fn set_rule_provenance<C, A>(
        &mut self,
        rule: usize,
        span: Option<HirSpan>,
        conditions: C,
        actions: A,
    ) -> Result<(), IntegrationError>
    where
        C: IntoIterator<Item = Option<HirSpan>>,
        A: IntoIterator<Item = Option<HirSpan>>,
    {
        self.program
            .set_rule_span(rule, self.workshop_span(span)?)
            .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
        for (index, span) in conditions.into_iter().enumerate() {
            self.program
                .set_condition_span(rule, index, self.workshop_span(span)?)
                .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
        }
        for (index, span) in actions.into_iter().enumerate() {
            self.program
                .set_action_span(rule, index, self.workshop_span(span)?)
                .map_err(|error| IntegrationError::new("provenance", error.to_string(), span))?;
        }
        Ok(())
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
            | Stmt::Return { .. }
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
        "playerDealtKnockback" => PlayerEventKind::DealtKnockback,
        "playerDied" => PlayerEventKind::Died,
        "playerEarnedElimination" => PlayerEventKind::EarnedElimination,
        "playerJoined" => PlayerEventKind::Joined,
        "playerLeft" => PlayerEventKind::Left,
        "playerReceivedHealing" => PlayerEventKind::ReceivedHealing,
        "playerReceivedKnockback" => PlayerEventKind::ReceivedKnockback,
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
    workshop_rs::format::format_number(value)
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

fn split_format_chunks(text: &str, arg_count: usize) -> Option<Vec<(String, Vec<usize>)>> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut indices = Vec::new();
    let mut pending = String::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let Some(open_rel) = text[cursor..].find('{') else {
            pending.push_str(&text[cursor..]);
            break;
        };
        let open = cursor + open_rel;
        let Some(close_rel) = text[open + 1..].find('}') else {
            pending.push_str(&text[cursor..]);
            break;
        };
        let close = open + 1 + close_rel;
        let marker = &text[open + 1..close];
        let Ok(index) = marker.parse::<usize>() else {
            pending.push_str(&text[cursor..=close]);
            cursor = close + 1;
            continue;
        };
        if index >= arg_count {
            return None;
        }
        pending.push_str(&text[cursor..open]);
        if indices.len() == 3 && !indices.contains(&index) {
            chunks.push((current, indices));
            current = String::new();
            indices = Vec::new();
        }
        current.push_str(&pending);
        pending.clear();
        let local = if let Some(local) = indices.iter().position(|candidate| *candidate == index) {
            local
        } else {
            indices.push(index);
            indices.len() - 1
        };
        current.push('{');
        current.push_str(&local.to_string());
        current.push('}');
        cursor = close + 1;
    }
    current.push_str(&pending);
    if current.is_empty() && chunks.is_empty() {
        return Some(vec![(text.to_string(), Vec::new())]);
    }
    chunks.push((current, indices));
    Some(chunks)
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

fn modify_op_from_str(op: &str) -> Option<ModifyOp> {
    match op {
        "+" => Some(ModifyOp::Add),
        "-" => Some(ModifyOp::Subtract),
        "*" => Some(ModifyOp::Multiply),
        "/" => Some(ModifyOp::Divide),
        "%" => Some(ModifyOp::Modulo),
        "**" => Some(ModifyOp::RaiseToPower),
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
