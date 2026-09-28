use super::*;

impl<'a> Lowering<'a> {
    fn translation_helper_index(
        &self,
        reserved: &HashSet<u32>,
    ) -> Result<Option<u32>, IntegrationError> {
        let Some(translations) = self.hir.preprocessing.translations.as_ref() else {
            return Ok(None);
        };
        self.free_global_index(
            reserved,
            translations.span,
            "no available global variable index remains for translations",
        )
    }

    fn helper_global_index(
        &self,
        reserved: &HashSet<u32>,
        directive: &str,
        message: &str,
    ) -> Result<Option<u32>, IntegrationError> {
        let Some(source) = self
            .hir
            .preprocessing
            .directives
            .iter()
            .find(|item| item.name == directive)
        else {
            return Ok(None);
        };
        self.free_global_index(reserved, source.span, message)
    }

    fn free_global_index(
        &self,
        reserved: &HashSet<u32>,
        span: Option<HirSpan>,
        message: &str,
    ) -> Result<Option<u32>, IntegrationError> {
        (0..=127)
            .rev()
            .find(|index| !reserved.contains(index))
            .map(Some)
            .ok_or_else(|| IntegrationError::new("index-exhausted", message, span))
    }

    pub(in crate::compiler) fn lower_declarations(&mut self) -> Result<(), IntegrationError> {
        let (implicit_globals, implicit_players) = implicit_default_variables(self.hir);
        let implicit_reserved = implicit_globals
            .keys()
            .map(|name| default_var_index(name).expect("implicit default variable names resolve"))
            .collect::<HashSet<_>>();
        let implicit_player_reserved = implicit_players
            .keys()
            .map(|name| implicit_player_index(name))
            .collect::<HashSet<_>>();
        let mut helper_reserved = implicit_reserved.clone();
        let mut globals = Vec::new();
        let mut players = Vec::new();
        let mut subroutines = Vec::new();
        for declaration in &self.hir.declarations {
            match declaration {
                hir::Declaration::GlobalVariable {
                    name, index, span, ..
                } => {
                    if let Some(index) = index {
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
                        helper_reserved.insert(*index);
                    }
                    globals.push((*index, *span));
                }
                hir::Declaration::PlayerVariable {
                    name, index, span, ..
                } => {
                    if let Some(index) = index {
                        for (implicit_name, implicit_span) in &implicit_players {
                            if implicit_player_index(implicit_name) == *index {
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
                    players.push((*index, *span));
                }
                hir::Declaration::Subroutine { index, span, .. } => {
                    subroutines.push((*index, *span));
                }
                hir::Declaration::Constant { .. } | hir::Declaration::Macro { .. } => {}
            }
        }

        let translation_helper_index = self.translation_helper_index(&helper_reserved)?;
        let mut global_reserved = implicit_reserved;
        if let Some(index) = translation_helper_index {
            helper_reserved.insert(index);
            global_reserved.insert(index);
        }
        let compression_alphabet_index = self.helper_global_index(
            &helper_reserved,
            "useVariableForCompressionAlphabet",
            "no available global variable index remains for the compression alphabet",
        )?;
        if let Some(index) = compression_alphabet_index {
            helper_reserved.insert(index);
            global_reserved.insert(index);
        }
        let empty_string_index = self.helper_global_index(
            &helper_reserved,
            "replaceEmptyStringByVariable",
            "no available global variable index remains for the empty-string replacement",
        )?;
        if let Some(index) = empty_string_index {
            helper_reserved.insert(index);
            global_reserved.insert(index);
        }
        let empty = HashSet::new();
        let global_indices = allocate_named_indices(
            self.hir
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    hir::Declaration::GlobalVariable { name, .. } => Some(name.as_str()),
                    _ => None,
                }),
            &mut globals,
            &global_reserved,
            "global variable",
        )?;
        let player_indices = allocate_named_indices(
            self.hir
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    hir::Declaration::PlayerVariable { name, .. } => Some(name.as_str()),
                    _ => None,
                }),
            &mut players,
            &implicit_player_reserved,
            "player variable",
        )?;
        let subroutine_indices = allocate_indices(&subroutines, &empty, "subroutine")?;
        let mut global_index = 0;
        let mut player_index = 0;
        let mut subroutine_index = 0;

        // Declared variables in source order (for duplicate detection and
        // initializer action order), then merged with the implicit default
        // variables and created in Workshop index order so the emitted
        // variable tables are reference-compatible.
        let mut planned_globals: Vec<(String, u32, Option<HirSpan>, Option<HirSpan>)> = Vec::new();
        let mut global_initializers = Vec::new();
        let mut planned_players: Vec<(String, u32, Option<HirSpan>, Option<HirSpan>)> = Vec::new();
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
                    if planned_globals
                        .iter()
                        .any(|(existing, ..)| existing == name)
                    {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate global variable '{name}'"),
                            *span,
                        ));
                    }
                    planned_globals.push((name.clone(), assigned, *span, *name_span));
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
                    if planned_players
                        .iter()
                        .any(|(existing, ..)| existing == name)
                    {
                        return Err(IntegrationError::new(
                            "symbol-collision",
                            format!("duplicate player variable '{name}'"),
                            *span,
                        ));
                    }
                    planned_players.push((name.clone(), assigned, *span, *name_span));
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
        if let Some(index) = compression_alphabet_index {
            planned_globals.push((COMPRESSION_ALPHABET_NAME.to_string(), index, None, None));
        }
        if let Some(index) = empty_string_index {
            planned_globals.push((EMPTY_STRING_NAME.to_string(), index, None, None));
        }
        planned_globals.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in planned_globals {
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

        planned_players.extend(
            implicit_players
                .iter()
                .map(|(name, span)| (name.clone(), implicit_player_index(name), *span, None)),
        );
        planned_players.sort_by_key(|(_, index, ..)| *index);
        for (name, assigned, span, name_span) in planned_players {
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

        let empty_string_initializer = empty_string_index
            .map(|_| {
                let variable = *self
                    .globals
                    .get(EMPTY_STRING_NAME)
                    .expect("empty string helper variable is created");
                let empty_array = self.push_call("emptyArray", Vec::new());
                let null = self.push_value(Value::Null);
                let value = self.push_call("charAt", vec![empty_array, null]);
                let action = self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value,
                });
                Ok(action)
            })
            .transpose()?;

        if has_directive(self.hir, "disableInspector") {
            let action = self.push_call_action("disableInspector", &[]);
            self.push_generated_rule("Disable inspector", Event::Global, vec![action])?;
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
                let action = self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value,
                });
                self.mark_action_origins(std::slice::from_ref(&action), translations.span);
                self.mark_action_argument_origins(action, [translations.span]);
                Ok(action)
            })
            .transpose()?;

        let compression_alphabet_initializer = compression_alphabet_index
            .map(|_| {
                let variable = *self
                    .globals
                    .get(COMPRESSION_ALPHABET_NAME)
                    .expect("compression alphabet variable is created");
                let value = self.lower_custom_string(compression_alphabet());
                let action = self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value,
                });
                Ok(action)
            })
            .transpose()?;

        let (uses_player_translation_var, no_detection_rule, no_tl_err) =
            self.translation_player_options();
        if uses_player_translation_var && !no_detection_rule {
            let translations = self
                .hir
                .preprocessing
                .translations
                .clone()
                .expect("player translation mode requires translations");
            self.lower_translation_detection_rule(no_tl_err, &translations)?;
        }

        if translation_initializer.is_some()
            || empty_string_initializer.is_some()
            || compression_alphabet_initializer.is_some()
            || !global_initializers.is_empty()
        {
            let mut actions = Vec::with_capacity(
                global_initializers.len()
                    + usize::from(translation_initializer.is_some())
                    + usize::from(empty_string_initializer.is_some())
                    + usize::from(compression_alphabet_initializer.is_some()),
            );
            if let Some(action) = translation_initializer {
                actions.push(action);
            }
            if let Some(action) = compression_alphabet_initializer {
                actions.push(action);
            }
            if let Some(action) = empty_string_initializer {
                actions.push(action);
            }
            for (name, init_expr, span, _target_span) in global_initializers {
                let variable = *self.globals.get(name).expect("declared global is created");
                let value = self.lower_value(init_expr)?;
                let action = self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value,
                });
                self.mark_action_origins(std::slice::from_ref(&action), span);
                self.mark_action_argument_origins(action, [init_expr.span().copied()]);
                actions.push(action);
            }
            let name = self.global_initializer_rule_name();
            self.push_generated_rule(&name, Event::Global, actions)?;
        }

        if uses_player_translation_var || !player_initializers.is_empty() {
            let mut actions = Vec::with_capacity(
                player_initializers.len() + usize::from(uses_player_translation_var),
            );
            if uses_player_translation_var {
                let variable = *self
                    .players
                    .get("__languageIndex__")
                    .expect("translation player variable is created");
                let player = self.push_value(Value::EventPlayer);
                let value = self.push_number(if no_tl_err { 0.1 } else { 1.1 });
                actions.push(self.push_action(Action::SetPlayerVariable {
                    player,
                    variable: self.player_names[variable].clone(),
                    value,
                }));
            }
            for (name, init_expr, span, _target_span) in player_initializers {
                let variable = *self
                    .players
                    .get(name)
                    .expect("declared player variable is created");
                let player = self.push_value(Value::EventPlayer);
                let value = self.lower_value(init_expr)?;
                let action = self.push_action(Action::SetPlayerVariable {
                    player,
                    variable: self.player_names[variable].clone(),
                    value,
                });
                self.mark_action_origins(std::slice::from_ref(&action), span);
                self.mark_action_argument_origins(action, [None, init_expr.span().copied()]);
                actions.push(action);
            }
            let name = self.player_initializer_rule_name();
            self.push_generated_rule(&name, Event::EachPlayer, actions)?;
        }

        Ok(())
    }

    fn push_generated_rule(
        &mut self,
        name: &str,
        event: Event,
        actions: Vec<ActionId>,
    ) -> Result<(), IntegrationError> {
        let rule = rule_from_parts(
            name.to_string(),
            false,
            event,
            Vec::new(),
            self.public_actions(&actions),
        );
        self.append_rule(rule, &actions, None, std::iter::empty())
    }

    pub(super) fn translation_player_options(&self) -> (bool, bool, bool) {
        let Some(directive) = self
            .hir
            .preprocessing
            .directives
            .iter()
            .find(|directive| directive.name == "translateWithPlayerVar")
        else {
            return (false, false, false);
        };
        let options = directive.value.as_deref().unwrap_or_default();
        (
            true,
            options
                .split_whitespace()
                .any(|option| option == "noDetectionRule"),
            options.split_whitespace().any(|option| option == "noTlErr"),
        )
    }

    fn lower_translation_detection_rule(
        &mut self,
        no_tl_err: bool,
        translations: &hir::TranslationState,
    ) -> Result<(), IntegrationError> {
        let variable = self
            .players
            .get("__languageIndex__")
            .copied()
            .expect("translation player variable is created");
        let player = self.push_value(Value::EventPlayer);
        let language = self.push_value(Value::PlayerVariable {
            player,
            variable: self.player_names[variable].clone(),
        });
        let initial = self.push_number(if no_tl_err { 0.1 } else { 1.1 });
        let has_spawned = self.push_call("hasSpawned", vec![player]);
        let is_dummy = self.push_call("isDummy", vec![player]);
        let false_value = self.push_value(Value::Bool(false));
        let not_dummy = self.push_call("==", vec![is_dummy, false_value]);
        let initial_language = self.push_call("==", vec![language, initial]);

        let facing = self.push_call("getFacingDirection", vec![player]);
        let append = self.push_action(Action::ModifyPlayerVariable {
            player,
            variable: self.player_names[variable].clone(),
            op: ModifyOp::AppendToArray,
            value: facing,
        });
        let ten = self.push_number(10.0);
        let direction_index = self.translation_language_index(translations)?;
        let horizontal = self.push_call("multiply", vec![ten, direction_index]);
        let vertical = self.push_number(5.0);
        let direction = self.push_call("directionFromAngles", vec![horizontal, vertical]);
        let turn_rate = self.push_number(999_999_999_999.0);
        let to_world = self.push_value(Value::Enum {
            value_type: "Relativity".to_string(),
            value: "TO_WORLD".to_string(),
        });
        let reevaluation = self.push_value(Value::Enum {
            value_type: "FacingReeval".to_string(),
            value: "DIRECTION_AND_TURN_RATE".to_string(),
        });
        let start_facing = self.push_call_action(
            "startFacing",
            &[player, direction, turn_rate, to_world, reevaluation],
        );

        let horizontal_angle = self.push_call("getHorizontalFacingAngle", vec![player]);
        let one_hundred = self.push_number(100.0);
        let horizontal_times_hundred =
            self.push_call("multiply", vec![horizontal_angle, one_hundred]);
        let nearest = self.push_value(Value::Enum {
            value_type: "Rounding".to_string(),
            value: "NEAREST".to_string(),
        });
        let rounded_horizontal =
            self.push_call("roundToInteger", vec![horizontal_times_hundred, nearest]);
        let thousand = self.push_number(1000.0);
        let modulo = self.push_call("modulo", vec![rounded_horizontal, thousand]);
        let zero = self.push_number(0.0);
        let modulo_zero = self.push_call("not", vec![modulo]);
        let vertical_angle = self.push_call("getVerticalFacingAngle", vec![player]);
        let vertical_difference = self.push_call("subtract", vec![vertical_angle, vertical]);
        let vertical_delta = self.push_call("absoluteValue", vec![vertical_difference]);
        let tolerance = self.push_number(0.01);
        let vertical_close = self.push_call("<", vec![vertical_delta, tolerance]);
        let wait_condition = self.push_call("and", vec![modulo_zero, vertical_close]);
        let timeout = self.push_number(15.0);
        let wait = self.push_call_action("waitUntil", &[wait_condition, timeout]);

        let ten_for_angle = self.push_number(10.0);
        let horizontal_divided = self.push_call("divide", vec![horizontal_angle, ten_for_angle]);
        let rounded_angle = self.push_call("roundToInteger", vec![horizontal_divided, nearest]);
        let vertical_difference = self.push_call("subtract", vec![vertical_angle, vertical]);
        let vertical_delta = self.push_call("absoluteValue", vec![vertical_difference]);
        let vertical_match = self.push_call("<", vec![vertical_delta, tolerance]);
        let one = self.push_number(1.0);
        let matched_language = self.push_call("multiply", vec![vertical_match, rounded_angle]);
        let language_value = self.push_call("max", vec![one, matched_language]);
        let set_index = self.push_call_action(
            "setPlayerVariableAtIndex",
            &[language, zero, language_value],
        );
        let stop_facing = self.push_call_action("stopFacing", &[player]);
        let last = self.push_call("lastOf", vec![language]);
        let set_facing = self.push_call_action("setFacing", &[player, last, to_world]);
        let finish = if no_tl_err {
            self.push_action(Action::ModifyPlayerVariable {
                player,
                variable: self.player_names[variable].clone(),
                op: ModifyOp::Subtract,
                value: one,
            })
        } else {
            let final_value = self.push_call("firstOf", vec![language]);
            self.push_action(Action::SetPlayerVariable {
                player,
                variable: self.player_names[variable].clone(),
                value: final_value,
            })
        };

        let actions = [
            append,
            start_facing,
            wait,
            set_index,
            stop_facing,
            set_facing,
            finish,
        ];
        let rule = rule_from_parts(
            "OverPy translation setup - Determine the player's language".to_string(),
            false,
            Event::EachPlayer,
            vec![
                workshop_rs::Condition::new(self.materialize_value(has_spawned)),
                workshop_rs::Condition::new(self.materialize_value(not_dummy)),
                workshop_rs::Condition::new(self.materialize_value(initial_language)),
            ],
            self.public_actions(&actions),
        );
        self.append_rule(rule, &actions, None, [None, None, None])?;
        Ok(())
    }
}
