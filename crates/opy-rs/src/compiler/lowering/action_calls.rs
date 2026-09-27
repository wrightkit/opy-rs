use super::*;

use super::super::blizzard_global;

fn is_cased_color_tag(text: &[char], index: usize) -> Option<usize> {
    let remaining = text[index..].iter().collect::<String>();
    let is_tag = remaining
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<fg"))
        || remaining
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("</fg>"));
    if !is_tag {
        return None;
    }
    remaining
        .find('>')
        .map(|offset| index + remaining[..=offset].chars().count())
}

fn cased_line(text: &str, text_count: usize) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    let mut text_without_tags = String::new();
    let mut plain_index = 0;
    while plain_index < characters.len() {
        if let Some(end) = is_cased_color_tag(&characters, plain_index) {
            plain_index = end;
        } else {
            text_without_tags.push(characters[plain_index]);
            plain_index += 1;
        }
    }
    let mut text_width = 0;
    let mut found_lowercase = false;
    for character in text_without_tags.chars() {
        if let Some(glyph) = blizzard_global::cased_glyph(character) {
            found_lowercase = true;
            if !matches!(character, 'i' | 'j' | 'l') {
                text_width = (glyph.lower_xmin - text_width).max(0);
                break;
            }
        } else {
            text_width += blizzard_global::width(character);
        }
    }
    if !found_lowercase {
        return vec![text.to_string(); text_count];
    }

    let mut outputs = vec![String::new(); text_count];
    let mut widths = vec![0; text_count];
    let mut text_index = 0;
    let mut last_character = None;
    let mut index = 0;
    while index < characters.len() {
        if let Some(end) = is_cased_color_tag(&characters, index) {
            let tag = characters[index..end].iter().collect::<String>();
            for output in &mut outputs {
                output.push_str(&tag);
            }
            text_index = (text_index + 1) % text_count;
            index = end;
            continue;
        }
        let character = characters[index];
        if let Some(glyph) = blizzard_global::cased_glyph(character) {
            text_index = (text_index + 1) % text_count;
            let padding = (text_width - widths[text_index] - glyph.lower_xmin + glyph.xmin).max(0);
            outputs[text_index].push_str(&blizzard_global::spaces(padding));
            widths[text_index] += padding;
            outputs[text_index].push_str(glyph.lower);
            widths[text_index] += glyph.lower_width;
            last_character = Some(character);
        } else if character != ' ' {
            if outputs[text_index].is_empty()
                || last_character
                    .and_then(blizzard_global::cased_glyph)
                    .is_some()
                || last_character == Some(' ')
            {
                text_index = (text_index + 1) % text_count;
                let padding = (text_width - widths[text_index]).max(0);
                outputs[text_index].push_str(&blizzard_global::spaces(padding));
                widths[text_index] += padding;
            }
            outputs[text_index].push(character);
            widths[text_index] += blizzard_global::width(character);
            last_character = Some(character);
        } else {
            last_character = Some(character);
        }
        text_width += blizzard_global::cased_glyph(character)
            .map_or_else(|| blizzard_global::width(character), |glyph| glyph.width);
        index += 1;
    }
    let maximum = widths
        .iter()
        .copied()
        .max()
        .unwrap_or_default()
        .max(text_width);
    for (output, width) in outputs.iter_mut().zip(widths) {
        output.push_str(&blizzard_global::spaces(maximum - width));
    }
    outputs
}

impl<'a> Lowering<'a> {
    pub(super) fn lower_cased_progress_bar(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<Vec<ActionId>, IntegrationError> {
        let [
            Expr::Number {
                value: text_count, ..
            },
            visible_to,
            Expr::String { value: text, .. },
            position,
            scale,
            clipping,
            text_color,
            reevaluation,
            spectators,
        ] = args
        else {
            return Err(self.unsupported(
                "createCasedProgressBarIwt requires a literal text count and text",
                span,
            ));
        };
        let text_count_value = *text_count;
        if !text_count_value.is_finite()
            || text_count_value.fract() != 0.0
            || !(2.0..=6.0).contains(&text_count_value)
        {
            return Err(self.unsupported(
                "createCasedProgressBarIwt text count must be between 2 and 6",
                span,
            ));
        }
        let text_count = text_count_value as usize;
        if args.iter().any(hir::visit::contains_random) {
            return Err(self.unsupported(
                "Cannot use random functions in createCasedProgressBarIwt",
                span,
            ));
        }
        let visible_to = self.lower_value(visible_to)?;
        let position = self.lower_value(position)?;
        let scale = self.lower_value(scale)?;
        let clipping = self.lower_value(clipping)?;
        let text_color = self.lower_value(text_color)?;
        let reevaluation = self.lower_value(reevaluation)?;
        let spectators = self.lower_value(spectators)?;
        let header_color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let texts = text
            .replace('\n', "  \n  ")
            .split('\n')
            .map(|line| {
                cased_line(line, text_count)
                    .into_iter()
                    .map(|line| format!("{line}\u{ad}"))
                    .collect::<Vec<_>>()
            })
            .reduce(|mut all, lines| {
                for (index, line) in lines.into_iter().enumerate() {
                    if index < all.len() {
                        all[index].push('\n');
                        all[index].push_str(&line);
                    }
                }
                all
            })
            .unwrap_or_else(|| vec![String::new(); text_count]);
        let mut actions = Vec::with_capacity(text_count);
        for (index, text) in texts.into_iter().enumerate() {
            let value = self.push_number(index as f64);
            let text = self.lower_custom_string(text);
            let values = self.normalize_contextual_arguments(
                "createProgressBarInWorldText",
                vec![
                    visible_to,
                    value,
                    text,
                    position,
                    scale,
                    clipping,
                    header_color,
                    text_color,
                    reevaluation,
                    spectators,
                ],
            );
            actions.push(self.push_call_action_with_spans(
                "createProgressBarInWorldText",
                &values,
                [None; 10],
            ));
        }
        Ok(actions)
    }

    pub(super) fn lower_action_call(
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
            let spans = args
                .iter()
                .map(|expr| expr.span().copied())
                .collect::<Vec<_>>();
            let args = self.lower_values(args)?;
            return Ok(self.push_call_action_with_spans(name, &args, spans));
        }
        let function = self
            .compiler
            .manifest
            .resolve_function(name)
            .ok_or_else(|| self.unsupported(format!("unknown action '{name}'"), span))?;
        if !matches!(function.kind, FunctionKind::Action) {
            return Err(self.unsupported(format!("'{name}' is not a generic OPY action"), span));
        }
        if matches!(function.id.as_str(), "async" | "startRule") {
            let [subroutine, behavior] = args else {
                return Err(self.unsupported(
                    format!(
                        "{} requires a subroutine and a start-rule behavior",
                        function.id
                    ),
                    span,
                ));
            };
            let subroutine_name = match subroutine {
                Expr::Call { name, args, .. } if args.is_empty() => name,
                _ => {
                    return Err(self.unsupported(
                        format!("{} requires a declared subroutine", function.id),
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
            let subroutine_span = subroutine.span().copied();
            let behavior_span = behavior.span().copied();
            let subroutine = self.push_value(Value::Subroutine(
                self.subroutine_names[subroutine_id].clone(),
            ));
            let behavior = self.lower_value(behavior)?;
            return Ok(self.push_call_action_with_spans(
                "startRule",
                &[subroutine, behavior],
                [subroutine_span, behavior_span],
            ));
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
            let spans = args
                .iter()
                .map(|expr| expr.span().copied())
                .chain(std::iter::once(None));
            let mut lowered = self.lower_values(args)?;
            let mut zero_vector = Vec::with_capacity(3);
            for value in [0.0, 0.0, 0.0] {
                zero_vector.push(self.push_number(value));
            }
            lowered.push(self.push_call("vector", zero_vector));
            let args = self.normalize_contextual_arguments("createDummyBot", lowered);
            return Ok(self.push_call_action_with_spans("createDummyBot", &args, spans));
        }
        let spans = args
            .iter()
            .map(|expr| expr.span().copied())
            .collect::<Vec<_>>();
        let args = self.lower_values(args)?;
        let catalog_id = if matches!(function.id.as_str(), "stopChasingVariable" | "stopChasing") {
            match args.first().map(|value| self.value(*value)) {
                Some(Value::GlobalVariable(_)) => "stopChasingGlobalVariable",
                Some(Value::PlayerVariable { .. }) => "stopChasingPlayerVariable",
                _ => {
                    return Err(self.unsupported(
                        "stopChasingVariable requires a global or player variable",
                        span,
                    ));
                }
            }
        } else {
            function.catalog_id.as_deref().ok_or_else(|| {
                self.unsupported(
                    format!(
                        "action '{}' requires a special lowering not in #46",
                        function.id
                    ),
                    span,
                )
            })?
        };
        let mut args = self.normalize_contextual_arguments(catalog_id, args);
        self.apply_replacements_to_values(catalog_id, &mut args, span);
        self.optimize_wait_duration(catalog_id, &mut args, span);
        Ok(self.push_call_action_with_spans(catalog_id, &args, spans))
    }

    fn optimize_wait_duration(
        &mut self,
        catalog_id: &str,
        args: &mut [ValueId],
        span: Option<HirSpan>,
    ) {
        const DEFAULT_WAIT_SECONDS: f64 = 0.016;

        let optimization = self.optimization_state_at(span.as_ref());
        if catalog_id != "wait" || !optimization.enabled || !optimization.for_size {
            return;
        }
        let Some(duration) = args.first().copied() else {
            return;
        };
        match self.value(duration) {
            Value::Number(value) if *value <= DEFAULT_WAIT_SECONDS => {
                let value = self.push_value(Value::Bool(false));
                args[0] = self.normalize_contextual_argument(catalog_id, 0, value);
            }
            Value::Number(value) if *value == 1.0 => {
                let value = self.push_value(Value::Bool(true));
                args[0] = self.normalize_contextual_argument(catalog_id, 0, value);
            }
            _ => {}
        }
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
        let visible_to_span = visible_to.span().copied();
        let visible_to = self.lower_hud_visible_to(visible_to)?;
        let mut text_slots = [
            self.push_value(Value::Null),
            self.push_value(Value::Null),
            self.push_value(Value::Null),
        ];
        let text_value = self.lower_text_value(text)?;
        text_slots[text_slot - 1] = if matches!(self.value(text_value), Value::String(_)) {
            self.push_call("customString", vec![text_value])
        } else {
            text_value
        };
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
        let text_span = text.span().copied();
        let color_span = color.span().copied();
        Ok(self.push_call_action_with_spans(
            "createHudText",
            &args,
            [
                visible_to_span,
                (text_slot == 1).then_some(text_span).flatten(),
                (text_slot == 2).then_some(text_span).flatten(),
                (text_slot == 3).then_some(text_span).flatten(),
                position.span().copied(),
                sort_order.span().copied(),
                (text_slot == 1).then_some(color_span).flatten(),
                (text_slot == 2).then_some(color_span).flatten(),
                (text_slot == 3).then_some(color_span).flatten(),
                reevaluation.span().copied(),
                spectators.span().copied(),
            ],
        ))
    }

    fn lower_hud_visible_to(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        if let Expr::Call { name, args, .. } = expr {
            if name == "getAllPlayers" && args.is_empty() {
                return Ok(self.lower_all_players());
            }
        }
        self.lower_value(expr)
    }

    pub(super) fn lower_all_players(&mut self) -> ValueId {
        let all_teams = self.push_value(Value::Enum {
            value_type: "Team".to_string(),
            value: "ALL".to_string(),
        });
        self.push_call("allPlayers", vec![all_teams])
    }

    pub(super) fn lower_receiver_action_call(
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
                ModifyOp::RemoveFromArrayByValue
            };
            let value_span = value.span().copied();
            let value = self.lower_value(value)?;
            return match receiver {
                Expr::GlobalVar {
                    name,
                    span: target_span,
                } => {
                    let variable = self.global_variable_id(name, *target_span)?;
                    let action = self.push_action(Action::ModifyGlobalVariable {
                        variable: self.global_names[variable].clone(),
                        op,
                        value,
                    });
                    self.mark_action_argument_origins(action, [value_span]);
                    Ok(action)
                }
                Expr::PlayerVar {
                    player,
                    name,
                    span: target_span,
                    ..
                } => {
                    let variable = self.player_variable_id(name, *target_span)?;
                    let player_span = player.span().copied();
                    let player = self.lower_value(player)?;
                    let action = self.push_action(Action::ModifyPlayerVariable {
                        player,
                        variable: self.player_names[variable].clone(),
                        op,
                        value,
                    });
                    self.mark_action_argument_origins(action, [player_span, value_span]);
                    Ok(action)
                }
                Expr::Index { array, index, .. } => {
                    let op_name = if function.id == "append" {
                        "appendToArray"
                    } else {
                        "removeFromArray"
                    };
                    let op_node = self.push_call(op_name, Vec::new());
                    let index_span = index.span().copied();
                    let target_span = array.span().copied();
                    let index = self.lower_value(index)?;
                    match array.as_ref() {
                        Expr::GlobalVar {
                            name,
                            span: array_span,
                        } => {
                            let variable = self.global_variable_id(name, *array_span)?;
                            let variable = self.push_value(Value::GlobalVariable(
                                self.global_names[variable].clone(),
                            ));
                            let args = self.normalize_contextual_arguments(
                                "modifyGlobalVariableAtIndex",
                                vec![variable, index, op_node, value],
                            );
                            let action =
                                self.push_call_action("modifyGlobalVariableAtIndex", &args);
                            self.mark_action_argument_origins(
                                action,
                                [target_span, index_span, None, value_span],
                            );
                            return Ok(action);
                        }
                        Expr::PlayerVar {
                            player,
                            name,
                            span: array_span,
                            ..
                        } => {
                            let variable = self.player_variable_id(name, *array_span)?;
                            let player = self.lower_value(player)?;
                            let variable = self.push_value(Value::PlayerVariable {
                                player,
                                variable: self.player_names[variable].clone(),
                            });
                            let args = self.normalize_contextual_arguments(
                                "modifyPlayerVariableAtIndex",
                                vec![variable, index, op_node, value],
                            );
                            let action =
                                self.push_call_action("modifyPlayerVariableAtIndex", &args);
                            self.mark_action_argument_origins(
                                action,
                                [target_span, index_span, None, value_span],
                            );
                            return Ok(action);
                        }
                        _ => {
                            return Err(self.unsupported(
                                format!(
                                    "{} requires a global or player variable receiver",
                                    function.id
                                ),
                                receiver.span().copied().or(span),
                            ));
                        }
                    }
                }
                _ => Err(self.unsupported(
                    format!(
                        "{} requires a global or player variable receiver",
                        function.id
                    ),
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
        let argument_spans = std::iter::once(receiver.span().copied())
            .chain(args.iter().map(|arg| arg.span().copied()))
            .collect::<Vec<_>>();
        let mut lowered = Vec::with_capacity(args.len() + 1);
        lowered.push(self.lower_value(receiver)?);
        lowered.extend(self.lower_values(args.iter())?);
        let mut args = self.normalize_contextual_arguments(catalog_id, lowered);
        self.apply_replacements_to_values(catalog_id, &mut args, span);
        Ok(self.push_call_action_with_spans(catalog_id.clone(), &args, argument_spans))
    }
}
