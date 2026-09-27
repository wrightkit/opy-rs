use super::*;

impl<'a> Lowering<'a> {
    pub(in crate::compiler) fn lower_rules(&mut self) -> Result<(), IntegrationError> {
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
        for rule in &mut self.program.rules {
            rule.name = escape_bad_words(&rule.name);
        }
        Ok(())
    }

    fn lower_rule(&mut self, rule: &hir::Rule) -> Result<(), IntegrationError> {
        self.reject_rule_metadata(rule)?;
        let event = self.lower_event(&rule.event, &rule.annotations)?;
        let mut condition_exprs = Vec::new();
        for expr in &rule.conditions {
            Self::split_rule_condition(expr, &mut condition_exprs);
        }
        let conditions = condition_exprs.iter().copied();
        let conditions = self.lower_values(conditions)?;
        let previous_conditions = self.current_rule_conditions.replace(conditions.clone());
        let lowered_actions = self.lower_actions(&rule.actions, None);
        self.current_rule_conditions = previous_conditions;
        let mut actions = Vec::new();
        actions.extend(lowered_actions?);
        let optimization = self.optimization_state_at(rule.span.as_ref());
        if optimization.enabled
            && !rule.delimiter
            && !self.has_meaningful_rule_action(&self.useful_actions(&actions), &event)
        {
            return Ok(());
        }
        let elide_noop_switch = actions.is_empty()
            && rule.actions.len() == 1
            && matches!(rule.actions.first(), Some(Stmt::Switch { .. }));
        if elide_noop_switch && !rule.disabled {
            return Ok(());
        }
        let rule_index = self.program.rules.len();
        self.program.rules.push(rule_from_parts(
            rule.name.clone(),
            rule.disabled,
            event,
            conditions
                .iter()
                .zip(&condition_exprs)
                .map(|(value, expr)| {
                    let mut condition = self.materialize_value(*value);
                    split_all(&mut condition);
                    let optimization = self.optimization_state_at(expr.span());
                    if optimization.enabled && optimization.for_size {
                        SizeOptimizer::new(self.compiler).condition(&mut condition);
                    }
                    let mut condition = OperatorOptimizer::new(self.compiler, optimization.strict)
                        .wrap_condition(condition);
                    trim_numbers(&mut condition);
                    workshop_rs::Condition::new(condition)
                })
                .collect(),
            self.public_actions(&actions),
        ));
        let action_provenance = self.action_provenance(&actions);
        self.set_rule_provenance(
            rule_index,
            rule.span,
            condition_exprs.iter().map(|expr| expr.span().copied()),
            action_provenance,
        )?;
        Ok(())
    }

    fn split_rule_condition<'expr>(expr: &'expr Expr, conditions: &mut Vec<&'expr Expr>) {
        match expr {
            Expr::Binary {
                op, left, right, ..
            } if op == "and" => {
                Self::split_rule_condition(left, conditions);
                Self::split_rule_condition(right, conditions);
            }
            Expr::Binary {
                op, left, right, ..
            } if op == "=="
                && matches!(right.as_ref(), Expr::Bool { value: true, .. })
                && matches!(left.as_ref(), Expr::Binary { op, .. } if op == "and") =>
            {
                Self::split_rule_condition(left, conditions);
            }
            _ => conditions.push(expr),
        }
    }

    fn has_meaningful_rule_action(&self, actions: &[ActionId], event: &Event) -> bool {
        actions
            .iter()
            .any(|action| match self.actions.get(*action) {
                Some(
                    Action::If { .. }
                    | Action::ElseIf { .. }
                    | Action::Else
                    | Action::While { .. }
                    | Action::End,
                )
                | None => false,
                Some(Action::Call { name, .. }) => match name.as_str() {
                    "abort" | "abortIf" | "break" | "continue" | "loop" | "loopIf" | "return"
                    | "skip" | "skipIf" => false,
                    "wait" => matches!(event, Event::Subroutine(_)),
                    _ => true,
                },
                Some(_) => true,
            })
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
        let event = Event::Subroutine(self.subroutine_names[subroutine].clone());
        if self.optimization_state_at(span.as_ref()).enabled
            && !self.has_meaningful_rule_action(&actions, &event)
        {
            return Ok(());
        }
        let rule_index = self.program.rules.len();
        self.program.rules.push(rule_from_parts(
            self.subroutine_rule_name(name),
            false,
            event,
            Vec::new(),
            self.public_actions(&actions),
        ));
        let action_provenance = self.action_provenance(&actions);
        self.set_rule_provenance(rule_index, span, std::iter::empty(), action_provenance)?;
        Ok(())
    }

    fn reject_rule_metadata(&self, rule: &hir::Rule) -> Result<(), IntegrationError> {
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
                "Event" | "Condition" | "Team" | "Slot" | "Hero" | "Disabled" | "Delimiter"
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

    pub(super) fn global_initializer_rule_name(&self) -> String {
        directive_value(self.hir, "globalvarInitRuleName")
            .map(str::to_string)
            .unwrap_or_else(|| {
                crate::lower::render_generated_rule_name(
                    "Initialize global variables",
                    &self.hir.preprocessing,
                )
            })
    }

    pub(super) fn player_initializer_rule_name(&self) -> String {
        directive_value(self.hir, "playervarInitRuleName")
            .map(str::to_string)
            .unwrap_or_else(|| "Initialize player variables".to_string())
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
            let optimization = self.optimization_state_at(statement.span());
            if optimization.enabled
                && optimization.for_size
                && optimization.for_size_aggressive
                && index + 1 == statements.len()
                && let Stmt::If {
                    branches,
                    r#else: None,
                    span,
                } = statement
                && branches.len() == 1
            {
                let branch = &branches[0];
                let is_not_condition = matches!(
                    &*branch.condition,
                    Expr::Unary { op, .. } if op == "not"
                );
                let is_comparison = matches!(
                    &*branch.condition,
                    Expr::Binary { op, .. }
                        if matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=")
                );
                if (is_not_condition || is_comparison) && !branch.body.is_empty() {
                    let body = self.lower_actions(&branch.body, break_target)?;
                    if !body.is_empty() && (is_not_condition || branch.body.len() == 1) {
                        let condition = if is_not_condition {
                            let Expr::Unary { operand, .. } = &*branch.condition else {
                                unreachable!()
                            };
                            self.lower_value(operand)?
                        } else {
                            let condition = self.lower_value(&branch.condition)?;
                            self.push_call("not", vec![condition])
                        };
                        let distance = self.canonical_action_width(&body, *span)?;
                        let distance = self.push_number(distance as f64);
                        let skip = self.push_call_action("skipIf", &[condition, distance]);
                        self.mark_action_origins(std::slice::from_ref(&skip), *span);
                        actions.push(skip);
                        actions.extend(body);
                        index += 1;
                        continue;
                    }
                }
            }
            if let Some((condition, label, span)) = direct_conditional_goto(statement)
                && self
                    .visible_labels
                    .iter()
                    .any(|labels| labels.iter().any(|candidate| candidate == label))
            {
                let condition = self.lower_value(condition)?;
                let placeholder = self.push_number(0.0);
                let skip = self.push_call_action("skipIf", &[condition, placeholder]);
                self.mark_action_origins(std::slice::from_ref(&skip), span);
                actions.push(skip);
                self.deferred_gotos.push((skip, label.to_string(), span, 1));
                index += 1;
                continue;
            }
            if let Some((condition, offset, span)) = direct_conditional_dynamic_goto(statement) {
                let condition = self.lower_value(condition)?;
                let offset = self.lower_value(offset)?;
                let skip = self.push_call_action("skipIf", &[condition, offset]);
                self.mark_action_origins(std::slice::from_ref(&skip), span);
                actions.push(skip);
                let goto = self.push_call_action("skip", &[offset]);
                self.mark_action_origins(std::slice::from_ref(&goto), span);
                actions.push(goto);
                actions.push(self.push_call_action("disabledAbort", &[]));
                index += 1;
                continue;
            }
            match statement {
                Stmt::Label { name, .. } => {
                    self.resolve_deferred_gotos(&actions, name, actions.len())?;
                    labels.insert(name.clone(), actions.len());
                }
                Stmt::Goto {
                    label,
                    offset,
                    rule_start,
                    span,
                } => {
                    if *rule_start {
                        let loop_action = self.push_call_action("loop", &[]);
                        self.mark_action_origins(std::slice::from_ref(&loop_action), *span);
                        actions.push(loop_action);
                        index += 1;
                        continue;
                    }
                    let placeholder = self.push_number(0.0);
                    let action = self.push_call_action("skip", &[placeholder]);
                    self.mark_action_origins(std::slice::from_ref(&action), *span);
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
                        self.deferred_gotos.push((action, label, span, 0));
                        continue;
                    }
                    return Err(self.unsupported(format!("unknown goto label '{label}'"), span));
                };
                if target < position {
                    return Err(self
                        .unsupported("backward goto is not representable in canonical WIR", span));
                }
                let width = self.canonical_action_width(&actions[position + 1..target], span)?;
                self.push_number(width as f64)
            };
            let Some(Action::Call { args, .. }) = self.actions.get_mut(action) else {
                unreachable!("goto placeholder must be a call action")
            };
            args[0] = distance;
        }
        if self.visible_labels.len() == 1 && !self.deferred_gotos.is_empty() {
            let (_, label, span, _) = self.deferred_gotos.remove(0);
            return Err(self.unsupported(format!("unknown goto label '{label}'"), span));
        }
        self.visible_labels.pop();
        Ok(actions)
    }

    /// `if condition: return` and `if condition: loop()` lower to a single
    /// conditional action; with optimization a constant condition removes the
    /// condition entirely.
    fn lower_terminal_if(
        &mut self,
        branch: &hir::types::IfBranch,
        span: Option<HirSpan>,
    ) -> Result<Option<Vec<ActionId>>, IntegrationError> {
        let [child] = branch.body.as_slice() else {
            return Ok(None);
        };
        let is_loop_call = matches!(
            child,
            Stmt::Expr { expr, .. }
                if matches!(expr.as_ref(), Expr::Call { name, args, .. } if name == "loop" && args.is_empty())
        );
        let (unconditional, conditional, on_true, on_false) = match child {
            Stmt::Return { .. } => (
                "abort",
                "abortIf",
                "__abortIfConditionIsTrue__",
                "__abortIfConditionIsFalse__",
            ),
            Stmt::Goto {
                rule_start: true, ..
            } => (
                "loop",
                "loopIf",
                "loopIfConditionIsTrue",
                "__loopIfConditionIsFalse__",
            ),
            _ if is_loop_call => (
                "loop",
                "loopIf",
                "loopIfConditionIsTrue",
                "__loopIfConditionIsFalse__",
            ),
            _ => return Ok(None),
        };
        let is_rule_condition =
            |expr: &Expr| matches!(expr, Expr::Call { name, .. } if name == "ruleCondition");
        let rule_condition = match branch.condition.as_ref() {
            condition if is_rule_condition(condition) => Some(on_true),
            Expr::Unary { op, operand, .. }
                if op == "not" && is_rule_condition(operand.as_ref()) =>
            {
                Some(on_false)
            }
            _ => None,
        };
        if let Some(name) = rule_condition {
            return Ok(Some(vec![self.push_call_action(name, &[])]));
        }
        let optimization = self.optimization_state_at(span.as_ref());
        if !optimization.enabled {
            return Ok(None);
        }
        let condition = self.lower_value(&branch.condition)?;
        let materialized = self.materialize_value(condition);
        let operators = OperatorOptimizer::new(self.compiler, optimization.strict);
        let action = match operators.constant_truth(&materialized) {
            Some(false) => return Ok(Some(Vec::new())),
            Some(true) => self.push_call_action(unconditional, &[]),
            None => self.push_call_action(conditional, &[condition]),
        };
        self.mark_action_origins(std::slice::from_ref(&action), span);
        Ok(Some(vec![action]))
    }

    fn resolve_deferred_gotos(
        &mut self,
        actions: &[ActionId],
        label: &str,
        target: usize,
    ) -> Result<(), IntegrationError> {
        let deferred = std::mem::take(&mut self.deferred_gotos);
        let mut remaining = Vec::new();
        for (action, deferred_label, span, argument) in deferred {
            if deferred_label != label {
                remaining.push((action, deferred_label, span, argument));
                continue;
            }
            let Some(position) = actions.iter().position(|candidate| *candidate == action) else {
                remaining.push((action, deferred_label, span, argument));
                continue;
            };
            if target < position {
                return Err(
                    self.unsupported("backward goto is not representable in canonical WIR", span)
                );
            }
            // A deferred goto may sit inside a structured action, so this
            // slice is not necessarily a standalone valid action sequence.
            // The flat lowering stream has one action id per native action,
            // including the structural markers that the jump must cross.
            // Instructions dropped from the output must not widen the jump.
            let width = self.useful_actions(&actions[position + 1..target]).len();
            let distance = self.push_number(width as f64);
            let Some(Action::Call { args, .. }) = self.actions.get_mut(action) else {
                unreachable!("deferred goto placeholder must be a call action")
            };
            args[argument] = distance;
        }
        self.deferred_gotos = remaining;
        Ok(())
    }

    fn lower_action(
        &mut self,
        stmt: &Stmt,
        break_target: Option<BreakTarget>,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let result = match stmt {
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
                if let ([branch], None) = (branches.as_slice(), r#else)
                    && let Some(actions) = self.lower_terminal_if(branch, *span)?
                {
                    return Ok(actions);
                }
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
            } => self.lower_switch(value, arms, *span, break_target, None, false),
            Stmt::Delete { target, span } => self.lower_delete(target, *span).map(|action| vec![action]),
            Stmt::Continue { span } => Err(self.unsupported(
                "continue statements are only lowered while constructing a loop body",
                *span,
            )),
            Stmt::Goto {
                label,
                offset,
                rule_start,
                span,
            } => {
                if *rule_start {
                    Ok(vec![self.push_call_action("loop", &[])])
                } else if label.is_none() {
                    let offset = offset.as_ref().ok_or_else(|| {
                        self.unsupported("goto is missing a label or offset", *span)
                    })?;
                    let offset = self.lower_value(offset)?;
                    Ok(vec![self.push_call_action("skip", &[offset])])
                } else {
                    Err(self.unsupported(
                        "goto statements are not representable in canonical WIR",
                        *span,
                    ))
                }
            }
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
                Some(BreakTarget::Switch) => Ok(vec![self.push_action(Action::Else)]),
                None => Err(self.unsupported(
                    "break has no enclosing canonical loop or switch",
                    *span,
                )),
            },
            Stmt::Return { span: _ } => Ok(vec![self.push_call_action("abort", &[])]),
            Stmt::Expr { expr, span } => match expr.as_ref() {
                Expr::Call {
                    name,
                    args,
                    debug_source,
                    ..
                } => {
                    if name == "disableInspector" && args.is_empty() {
                        Ok(vec![self.push_call_action("disableInspector", &[])])
                    } else if name == "pass" && args.is_empty() {
                        Ok(Vec::new())
                    } else if name == "debug" && args.len() == 1 {
                        Ok(vec![self.lower_debug(&args[0], debug_source.as_deref())?])
                    } else if name == "print" && args.len() == 1 {
                        Ok(vec![self.lower_print(&args[0], *span)?])
                    } else if name == "createCasedProgressBarIwt" {
                        self.lower_cased_progress_bar(args, *span)
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
        };
        if let Ok(actions) = &result {
            self.mark_action_origins(actions, stmt.span().copied());
            self.mark_statement_argument_origins(stmt, actions);
        }
        result
    }

    fn mark_statement_argument_origins(&mut self, statement: &Stmt, actions: &[ActionId]) {
        match statement {
            Stmt::Assign { target, value, .. } => {
                let (target_span, index_span) = match &**target {
                    Expr::Index { array, index, .. } => {
                        (array.span().copied(), index.span().copied())
                    }
                    _ => (target.span().copied(), None),
                };
                let value_span = value.span().copied();
                let modified_value_span = match &**value {
                    Expr::Binary { right, .. } => right.span().copied(),
                    _ => value_span,
                };
                for action in actions {
                    let spans = match self.actions.get(*action) {
                        Some(Action::SetGlobalVariable { .. }) => vec![value_span],
                        Some(Action::ModifyGlobalVariable { .. }) => vec![modified_value_span],
                        Some(Action::SetPlayerVariable { .. }) => {
                            let player_span = match &**target {
                                Expr::PlayerVar { player, .. } => player.span().copied(),
                                _ => None,
                            };
                            vec![player_span, value_span]
                        }
                        Some(Action::ModifyPlayerVariable { .. }) => {
                            let player_span = match &**target {
                                Expr::PlayerVar { player, .. } => player.span().copied(),
                                _ => None,
                            };
                            vec![player_span, modified_value_span]
                        }
                        Some(Action::Call { name, .. })
                            if name == "setGlobalVariableAtIndex"
                                || name == "setPlayerVariableAtIndex" =>
                        {
                            vec![target_span, index_span, value_span]
                        }
                        Some(Action::Call { name, .. })
                            if name == "modifyGlobalVariableAtIndex"
                                || name == "modifyPlayerVariableAtIndex" =>
                        {
                            vec![target_span, index_span, None, modified_value_span]
                        }
                        _ => continue,
                    };
                    self.mark_action_argument_origins(*action, spans);
                }
            }
            Stmt::If { branches, .. } => {
                let mut depth = 0usize;
                let mut branch = 0usize;
                for action in actions {
                    match self.actions.get(*action) {
                        Some(Action::If { .. }) => {
                            if depth == 0 {
                                self.mark_action_argument_origins(
                                    *action,
                                    [branches
                                        .first()
                                        .and_then(|branch| branch.condition.span().copied())],
                                );
                            }
                            depth += 1;
                        }
                        Some(Action::ElseIf { .. }) if depth == 1 => {
                            branch += 1;
                            self.mark_action_argument_origins(
                                *action,
                                [branches
                                    .get(branch)
                                    .and_then(|branch| branch.condition.span().copied())],
                            );
                        }
                        Some(Action::End) => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                }
            }
            Stmt::While { condition, .. } => {
                if let Some(action) = actions.first() {
                    if matches!(self.actions.get(*action), Some(Action::While { .. })) {
                        self.mark_action_argument_origins(*action, [condition.span().copied()]);
                    }
                }
            }
            Stmt::For {
                variable, iterable, ..
            } => {
                let range_spans = match &**iterable {
                    Expr::Call { args, .. } => match args.as_slice() {
                        [stop] => vec![None, stop.span().copied(), None],
                        [start, stop] => {
                            vec![start.span().copied(), stop.span().copied(), None]
                        }
                        [start, stop, step] => vec![
                            start.span().copied(),
                            stop.span().copied(),
                            step.span().copied(),
                        ],
                        _ => return,
                    },
                    _ => return,
                };
                let spans = match &**variable {
                    Expr::PlayerVar { player, .. } => std::iter::once(player.span().copied())
                        .chain(range_spans)
                        .collect::<Vec<_>>(),
                    _ => range_spans,
                };
                if let Some(action) = actions.first() {
                    if matches!(
                        self.actions.get(*action),
                        Some(Action::ForGlobalVariable { .. })
                            | Some(Action::ForPlayerVariable { .. })
                    ) {
                        self.mark_action_argument_origins(*action, spans);
                    }
                }
            }
            Stmt::DoWhile { condition, .. } => {
                if let Some(action) = actions.last() {
                    if matches!(
                        self.actions.get(*action),
                        Some(Action::Call { name, .. }) if name == "loopIf"
                    ) {
                        self.mark_action_argument_origins(*action, [condition.span().copied()]);
                    }
                }
            }
            Stmt::Delete { target, .. } => {
                let mut indices = Vec::new();
                let _ = indexed_target_parts(target, &mut indices);
                indices.reverse();
                for action in actions {
                    let spans = match self.actions.get(*action) {
                        Some(Action::SetGlobalVariable { .. })
                        | Some(Action::ModifyGlobalVariable { .. }) => {
                            vec![target.span().copied()]
                        }
                        Some(Action::SetPlayerVariable { .. })
                        | Some(Action::ModifyPlayerVariable { .. }) => {
                            vec![None, target.span().copied()]
                        }
                        Some(Action::Call { name, .. })
                            if name == "setGlobalVariableAtIndex"
                                || name == "setPlayerVariableAtIndex" =>
                        {
                            vec![
                                target.span().copied(),
                                indices.first().and_then(|index| index.span().copied()),
                                target.span().copied(),
                            ]
                        }
                        Some(Action::Call { name, .. })
                            if name == "modifyGlobalVariableAtIndex"
                                || name == "modifyPlayerVariableAtIndex" =>
                        {
                            vec![
                                target.span().copied(),
                                indices.first().and_then(|index| index.span().copied()),
                                None,
                                indices.last().and_then(|index| index.span().copied()),
                            ]
                        }
                        _ => continue,
                    };
                    self.mark_action_argument_origins(*action, spans);
                }
            }
            _ => {}
        }
    }

    fn lower_loop_body(&mut self, statements: &[Stmt]) -> Result<Vec<ActionId>, IntegrationError> {
        self.lower_loop_sequence_with_break_target(statements, &[], 0, BreakTarget::Loop)
    }

    fn lower_condition_chain(
        &mut self,
        conditions: &[&Expr],
    ) -> Result<Option<ValueId>, IntegrationError> {
        let mut lowered = None;
        for expression in conditions {
            let value = self.lower_value(expression)?;
            lowered = Some(match lowered {
                Some(left) => self.push_call("and", vec![left, value]),
                None => value,
            });
        }
        Ok(lowered)
    }

    fn lower_loop_sequence_with_break_target(
        &mut self,
        statements: &[Stmt],
        after: &[ActionId],
        structural_after: usize,
        break_target: BreakTarget,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let mut actions = Vec::new();
        let mut index = 0;
        while index < statements.len() {
            let statement = &statements[index];
            let tail = &statements[index + 1..];
            if let Some(conditions) = pure_continue_conditions(statement) {
                let tail = self.lower_loop_sequence_with_break_target(
                    tail,
                    after,
                    structural_after,
                    break_target,
                )?;
                let distance = self.canonical_action_width(&tail, statement.span().copied())?
                    + structural_after
                    + self.canonical_action_width(after, statement.span().copied())?;
                if distance > 0 {
                    let mut args = Vec::with_capacity(conditions.len() + 1);
                    if let Some(condition) = self.lower_condition_chain(&conditions)? {
                        args.push(condition);
                    }
                    let distance = self.push_number(distance as f64);
                    args.push(distance);
                    let skip = self.push_call_action(
                        if conditions.is_empty() {
                            "skip"
                        } else {
                            "skipIf"
                        },
                        &args,
                    );
                    self.mark_action_origins(
                        std::slice::from_ref(&skip),
                        statement.span().copied(),
                    );
                    actions.push(skip);
                }
                actions.extend(tail);
                return Ok(actions);
            }
            if contains_loop_continue(statement) {
                let tail = self.lower_loop_sequence_with_break_target(
                    tail,
                    after,
                    structural_after,
                    break_target,
                )?;
                let mut continuation_after = tail.clone();
                continuation_after.extend_from_slice(after);
                let lowered = if let Stmt::Switch { value, arms, span } = statement {
                    self.lower_switch(
                        value,
                        arms,
                        *span,
                        Some(break_target),
                        Some((&continuation_after, structural_after)),
                        false,
                    )?
                } else {
                    self.lower_if_with_loop_continue(
                        statement,
                        &continuation_after,
                        structural_after,
                        break_target,
                    )?
                };
                self.mark_action_origins(&lowered, statement.span().copied());
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
                    let middle = self.lower_loop_sequence_with_break_target(
                        &statements[index + 1..target],
                        after,
                        structural_after,
                        break_target,
                    )?;
                    let suffix = self.lower_loop_sequence_with_break_target(
                        &statements[target + 1..],
                        after,
                        structural_after,
                        break_target,
                    )?;
                    let distance =
                        self.canonical_action_width(&middle, statement.span().copied())?;
                    let mut args = Vec::with_capacity(conditions.len() + 1);
                    if let Some(condition) = self.lower_condition_chain(&conditions)? {
                        args.push(condition);
                    }
                    args.push(self.push_number(distance as f64));
                    let skip = self.push_call_action(
                        if conditions.is_empty() {
                            "skip"
                        } else {
                            "skipIf"
                        },
                        &args,
                    );
                    self.mark_action_origins(
                        std::slice::from_ref(&skip),
                        statement.span().copied(),
                    );
                    actions.push(skip);
                    actions.extend(middle);
                    actions.extend(suffix);
                    return Ok(actions);
                }
            }
            if let Some((condition, label, span)) = direct_conditional_goto(statement)
                && self
                    .visible_labels
                    .iter()
                    .any(|labels| labels.iter().any(|candidate| candidate == label))
            {
                let condition = self.lower_value(condition)?;
                let placeholder = self.push_number(0.0);
                let skip = self.push_call_action("skipIf", &[condition, placeholder]);
                self.mark_action_origins(std::slice::from_ref(&skip), span);
                actions.push(skip);
                self.deferred_gotos.push((skip, label.to_string(), span, 1));
                index += 1;
                continue;
            }
            if matches!(statement, Stmt::Label { .. }) {
                index += 1;
                continue;
            }
            actions.extend(self.lower_action(statement, Some(break_target))?);
            index += 1;
        }
        Ok(actions)
    }

    fn lower_if_with_loop_continue(
        &mut self,
        statement: &Stmt,
        after: &[ActionId],
        structural_after: usize,
        break_target: BreakTarget,
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
            let body = self.lower_loop_sequence_with_break_target(
                body,
                after,
                suffix_structural,
                break_target,
            )?;
            suffix.splice(0..0, body.iter().copied());
            suffix_structural += 1;
            lowered_else = Some(body);
        }
        for index in (0..branches.len()).rev() {
            let body = self.lower_loop_sequence_with_break_target(
                &branches[index].body,
                &suffix,
                suffix_structural,
                break_target,
            )?;
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
            if let Some(conditions) = pure_continue_conditions(statement) {
                let tail = self.lower_do_while_body(&statements[index + 1..])?;
                let action = if let Some(condition) = self.lower_condition_chain(&conditions)? {
                    self.push_call_action("loopIf", &[condition])
                } else {
                    self.push_call_action("loop", &[])
                };
                self.mark_action_origins(std::slice::from_ref(&action), statement.span().copied());
                actions.push(action);
                actions.extend(tail);
                return Ok(actions);
            }
            if contains_loop_continue(statement) {
                if let Stmt::Switch { value, arms, span } = statement {
                    let lowered = self.lower_switch(
                        value,
                        arms,
                        *span,
                        Some(BreakTarget::DoWhile),
                        None,
                        true,
                    )?;
                    self.mark_action_origins(&lowered, statement.span().copied());
                    actions.extend(lowered);
                    continue;
                }
                let Stmt::If {
                    branches, r#else, ..
                } = statement
                else {
                    unreachable!("continue-containing do-while statement must be an if")
                };
                let branches = branches
                    .iter()
                    .map(|branch| {
                        Ok((
                            self.lower_value(&branch.condition)?,
                            self.lower_do_while_body(&branch.body)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?;
                let else_body = r#else
                    .as_ref()
                    .map(|body| self.lower_do_while_body(body))
                    .transpose()?;
                let lowered = self.push_if_actions(branches, else_body);
                self.mark_action_origins(&lowered, statement.span().copied());
                actions.extend(lowered);
                continue;
            }
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
                let distance = self.push_number(distance as f64);
                let mut args = args;
                args.push(distance);
                let skip = self.push_call_action(name, &args);
                self.mark_action_origins(std::slice::from_ref(&skip), statement.span().copied());
                actions.push(skip);
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
        let number = |this: &mut Self, value: f64| this.push_number(value);
        match args.as_slice() {
            [stop] => Ok((
                number(self, 0.0),
                self.lower_value(stop)?,
                number(self, 1.0),
            )),
            [start, stop] => Ok((
                self.lower_value(start)?,
                self.lower_value(stop)?,
                number(self, 1.0),
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
        break_target: Option<BreakTarget>,
        loop_continue: Option<(&[ActionId], usize)>,
        do_while_continue: bool,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        if break_target.is_none()
            && arms.iter().all(|arm| match arm {
                SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => {
                    switch_body_is_noop(body)
                }
            })
        {
            return Ok(Vec::new());
        }
        let selector = self.lower_value(value)?;
        let mut case_values = Vec::new();
        let mut lowered_arms = Vec::with_capacity(arms.len());
        let mut has_default = false;
        let mut legacy_case_offsets = Vec::new();
        let mut legacy_offset = 0;
        let mut legacy_default_offset = None;

        let mut reverse_bodies = (loop_continue.is_some() && !do_while_continue).then(|| {
            (0..arms.len())
                .map(|_| None)
                .collect::<Vec<Option<LoweredSwitchBody>>>()
        });
        if let Some((outer_after, structural_after)) = loop_continue {
            let mut future = Vec::new();
            for index in (0..arms.len()).rev() {
                let body = match &arms[index] {
                    SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => body,
                };
                let mut after = future.clone();
                after.extend_from_slice(outer_after);
                let lowered = self.lower_switch_body(
                    body,
                    Some((&after, structural_after)),
                    do_while_continue,
                )?;
                let mut next_future = lowered.0.clone();
                next_future.extend_from_slice(&future);
                future = next_future;
                reverse_bodies.as_mut().unwrap()[index] = Some(lowered);
            }
        }
        for (index, arm) in arms.iter().enumerate() {
            let (value, (body, break_at)) = match arm {
                SwitchArm::Case { value, body, .. } => {
                    case_values.push(self.lower_value(value)?);
                    let lowered = if let Some(bodies) = reverse_bodies.as_mut() {
                        bodies[index].take().unwrap()
                    } else {
                        self.lower_switch_body(body, loop_continue, do_while_continue)?
                    };
                    (Some(value), lowered)
                }
                SwitchArm::Default { body, span } => {
                    if has_default {
                        return Err(
                            self.unsupported("a switch may contain at most one default arm", *span)
                        );
                    }
                    has_default = true;
                    legacy_default_offset = Some(legacy_offset);
                    let lowered = if let Some(bodies) = reverse_bodies.as_mut() {
                        bodies[index].take().unwrap()
                    } else {
                        self.lower_switch_body(body, loop_continue, do_while_continue)?
                    };
                    (None, lowered)
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

        let case_values = self.lower_array(case_values);
        if !use_shared_exit {
            let default_offset = legacy_default_offset.unwrap_or(legacy_offset);
            let offset_values = std::iter::once(default_offset)
                .chain(legacy_case_offsets)
                .map(|value| self.push_number(value as f64))
                .collect();
            let offsets = self.lower_array(offset_values);
            let skip = self.lower_switch_selector(selector, case_values, offsets);
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
        let skip = self.lower_switch_selector(selector, case_values, offsets);
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
            .map(|value| self.push_number(value as f64))
            .collect();
        let offset_values = self.lower_array(offset_values);
        let offset_value = self.value(offset_values).clone();
        let Some(node) = self.values.get_mut(offsets) else {
            unreachable!("switch offset placeholder must exist")
        };
        *node = offset_value;

        let Some(Action::Call { args, .. }) = self.actions.get_mut(skip) else {
            unreachable!("switch selector must be a call action")
        };
        let Some(selector_id) = args.first().copied() else {
            unreachable!("switch selector condition must be a value call")
        };
        let Some(Value::Call { args, .. }) = self.values.get_mut(selector_id) else {
            unreachable!("switch selector condition must be a value call")
        };
        args[0] = offsets;

        Ok(switch)
    }

    fn lower_switch_selector(
        &mut self,
        selector: ValueId,
        case_values: ValueId,
        offsets: ValueId,
    ) -> ActionId {
        let one = self.push_number(1.0);
        let index = self.push_call("indexOfArrayValue", vec![case_values, selector]);
        let case_offset = self.push_call("add", vec![one, index]);
        let skip_condition = self.push_call("valueInArray", vec![offsets, case_offset]);
        self.push_call_action("skip", &[skip_condition])
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
        loop_continue: Option<(&[ActionId], usize)>,
        do_while_continue: bool,
    ) -> Result<LoweredSwitchBody, IntegrationError> {
        let mut actions = Vec::new();
        let break_index = statements
            .iter()
            .position(|statement| matches!(statement, Stmt::Break { .. }));
        let body_end = break_index.unwrap_or(statements.len());
        if do_while_continue {
            actions.extend(self.lower_do_while_body(&statements[..body_end])?);
        } else if let Some((after, structural_after)) = loop_continue {
            actions.extend(self.lower_loop_sequence_with_break_target(
                &statements[..body_end],
                after,
                structural_after + 1,
                BreakTarget::Switch,
            )?);
        } else {
            for statement in &statements[..body_end] {
                actions.extend(self.lower_action(statement, Some(BreakTarget::Switch))?);
            }
        }
        let mut break_at = None;
        if let Some(index) = break_index {
            let Stmt::Break { span } = &statements[index] else {
                unreachable!("switch break index must point to a break")
            };
            if statements[index + 1..]
                .iter()
                .any(|statement| matches!(statement, Stmt::Break { .. }))
            {
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
            for statement in statements[index + 1..].iter() {
                actions.extend(self.lower_action(statement, Some(BreakTarget::Switch))?);
            }
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
        program.rules.push(rule_from_parts(
            "action layout".to_string(),
            false,
            workshop_rs::Event::Global,
            Vec::new(),
            public_actions.clone(),
        ));
        workshop_rs::emitter::action_width(
            &program,
            self.compiler.catalog,
            &Locale::new("en-US"),
            &public_actions,
        )
        .map(|layout| layout.width)
        .map_err(|error| {
            let span = fallback_span;
            IntegrationError::new("workshop-action-layout", error.to_string(), span)
        })
    }
}
