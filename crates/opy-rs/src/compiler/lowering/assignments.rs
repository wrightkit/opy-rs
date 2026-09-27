use super::*;

impl<'a> Lowering<'a> {
    pub(super) fn lower_delete(
        &mut self,
        target: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let mut indices = Vec::new();
        let Some(root) = indexed_target_parts(target, &mut indices) else {
            return Err(self.unsupported(
                "delete statements require an indexed global or player variable",
                span,
            ));
        };
        if indices.len() > 4 {
            return Err(self.unsupported("Cannot delete index of 4d array", span));
        }
        indices.reverse();
        if hir::visit::has_random_nested_delete(root, &indices) {
            return Err(self.unsupported(
                "Cannot delete from nested array with a random outer or middle index",
                span,
            ));
        }
        let (root_value, action_name) = match root {
            Expr::GlobalVar {
                name,
                span: target_span,
            } => {
                let variable = *self.globals.get(name).ok_or_else(|| {
                    self.unsupported(format!("unknown global variable '{name}'"), *target_span)
                })?;
                let root_value =
                    self.push_value(Value::GlobalVariable(self.global_names[variable].clone()));
                (root_value, "modifyGlobalVariableAtIndex")
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
                    player,
                    variable: self.player_names[variable].clone(),
                });
                (value, "modifyPlayerVariableAtIndex")
            }
            _ => {
                return Err(self.unsupported(
                    "delete statements are only representable for global or player variables",
                    target.span().copied(),
                ));
            }
        };
        let index = self.lower_value(indices[0])?;
        if indices.len() == 1 {
            let op = ModifyOp::RemoveFromArrayByIndex;
            return Ok(if action_name == "modifyGlobalVariableAtIndex" {
                let variable = match self.values.get(root_value) {
                    Some(Value::GlobalVariable(variable)) => variable.clone(),
                    _ => unreachable!("global delete root must be a global variable value"),
                };
                self.push_action(Action::ModifyGlobalVariable {
                    variable,
                    op,
                    value: index,
                })
            } else {
                let (player, variable) = match self.values.get(root_value) {
                    Some(Value::PlayerVariable { player, variable }) => (*player, variable.clone()),
                    _ => unreachable!("player delete root must be a player variable value"),
                };
                self.push_action(Action::ModifyPlayerVariable {
                    player,
                    variable,
                    op,
                    value: index,
                })
            });
        }

        let op = self.push_call("removeFromArrayByIndex", Vec::new());
        if indices.len() == 2 {
            let inner_index = self.lower_value(indices[1])?;
            let args = self.normalize_contextual_arguments(
                action_name,
                vec![root_value, index, op, inner_index],
            );
            return Ok(self.push_call_action(action_name, &args));
        }

        let outer_array = self.lower_indexed_read(root_value, indices[0], index);
        if indices.len() == 4 {
            let replacement = self.rebuild_deleted_array(outer_array, &indices[1..])?;
            let action_name = if action_name == "modifyGlobalVariableAtIndex" {
                "setGlobalVariableAtIndex"
            } else {
                "setPlayerVariableAtIndex"
            };
            let args = self
                .normalize_contextual_arguments(action_name, vec![root_value, index, replacement]);
            return Ok(self.push_call_action(action_name, &args));
        }
        let inner_index = self.lower_value(indices[1])?;
        let row = self.lower_indexed_read(outer_array, indices[1], inner_index);
        let leaf_index = self.lower_value(indices[2])?;
        let current_index = self.push_call("currentArrayIndex", Vec::new());
        let condition = self.push_call("!=", vec![current_index, leaf_index]);
        let filtered = self.push_call("filteredArray", vec![row, condition]);
        let replacement = if let Some(number) = hir::visit::literal_number(indices[1]) {
            let middle = self.lower_array(vec![filtered]);
            let maximum = self.push_number(999_999_999_999.0);
            let suffix_start = self.push_number(number + 1.0);
            let suffix = self.push_call("slice", vec![outer_array, suffix_start, maximum]);
            if number == 0.0 {
                self.push_call("appendToArray", vec![middle, suffix])
            } else {
                let zero = self.push_number(0.0);
                let prefix = self.push_call("slice", vec![outer_array, zero, inner_index]);
                let with_replacement = self.push_call("appendToArray", vec![prefix, middle]);
                self.push_call("appendToArray", vec![with_replacement, suffix])
            }
        } else {
            self.replace_array_element(outer_array, inner_index, filtered)
        };
        let action_name = if action_name == "modifyGlobalVariableAtIndex" {
            "setGlobalVariableAtIndex"
        } else {
            "setPlayerVariableAtIndex"
        };
        let args =
            self.normalize_contextual_arguments(action_name, vec![root_value, index, replacement]);
        Ok(self.push_call_action(action_name, &args))
    }

    pub(super) fn lower_assign(
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
                return self.lower_nested_indexed_assign(root, &indices, target, value);
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
                    && matches!(left.as_ref(), Expr::GlobalVar { name: left_name, .. } if left_name == name)
                    && let Some((modify_op, _)) = modify_operator(op)
                {
                    let value = self.lower_value(right)?;
                    return Ok(self.push_action(Action::ModifyGlobalVariable {
                        variable: self.global_names[variable].clone(),
                        op: modify_op,
                        value,
                    }));
                }
                let val = self.lower_value(value)?;
                Ok(self.push_action(Action::SetGlobalVariable {
                    variable: self.global_names[variable].clone(),
                    value: val,
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
                    && matches!(left.as_ref(), Expr::PlayerVar { player: left_player, name: left_name, .. } if left_name == name && left_player.as_ref() == player.as_ref())
                    && let Some((modify_op, _)) = modify_operator(op)
                {
                    let value = self.lower_value(right)?;
                    return Ok(self.push_action(Action::ModifyPlayerVariable {
                        player: player_val,
                        variable: self.player_names[variable].clone(),
                        op: modify_op,
                        value,
                    }));
                }
                let val = self.lower_value(value)?;
                Ok(self.push_action(Action::SetPlayerVariable {
                    player: player_val,
                    variable: self.player_names[variable].clone(),
                    value: val,
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
                    self.lower_indexed_assignment(
                        target,
                        var_node,
                        index_val,
                        value,
                        ("setGlobalVariableAtIndex", "modifyGlobalVariableAtIndex"),
                    )
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
                        player: player_val,
                        variable: self.player_names[variable].clone(),
                    });
                    let index_val = self.lower_value(index)?;
                    self.lower_indexed_assignment(
                        target,
                        var_node,
                        index_val,
                        value,
                        ("setPlayerVariableAtIndex", "modifyPlayerVariableAtIndex"),
                    )
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

    fn lower_indexed_assignment(
        &mut self,
        target: &Expr,
        variable: ValueId,
        index_value: ValueId,
        value: &Expr,
        actions: (&str, &str),
    ) -> Result<ActionId, IntegrationError> {
        let Expr::Index { array, index, .. } = target else {
            unreachable!("indexed assignment target was matched before lowering")
        };
        let (set_action, modify_action) = actions;
        if let Expr::Binary {
            op, left, right, ..
        } = value
            && let Expr::Index {
                array: left_array,
                index: left_index,
                ..
            } = left.as_ref()
            && left_array.as_ref() == array.as_ref()
            && left_index.as_ref() == index.as_ref()
            && let Some((_, call_name)) = modify_operator(op)
        {
            let operator = self.push_call(call_name, Vec::new());
            let value = self.lower_value(right)?;
            let args = self.normalize_contextual_arguments(
                modify_action,
                vec![variable, index_value, operator, value],
            );
            return Ok(self.push_call_action(modify_action, &args));
        }

        let value = self.lower_value(value)?;
        let args =
            self.normalize_contextual_arguments(set_action, vec![variable, index_value, value]);
        Ok(self.push_call_action(set_action, &args))
    }

    fn lower_nested_indexed_assign(
        &mut self,
        root: &Expr,
        indices: &[&Expr],
        target: &Expr,
        value: &Expr,
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
                    player: player_value,
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
        let outer_array = self.lower_indexed_read(root_value, indices[0], outer_index);
        let replacement = self.rebuild_indexed_value(outer_array, &indices[1..], target, value)?;
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
                && let Some((_, call_name)) = modify_operator(op)
            {
                let current = self.lower_indexed_read(array, index, index_value);
                let right = self.lower_value(right)?;
                self.push_call(call_name, vec![current, right])
            } else {
                self.lower_value(value)?
            }
        } else {
            let child = self.lower_indexed_read(array, index, index_value);
            self.rebuild_indexed_value(child, &indices[1..], target, value)?
        };
        Ok(self.replace_array_element(array, index_value, replacement))
    }

    fn lower_indexed_read(
        &mut self,
        array: ValueId,
        index: &Expr,
        index_value: ValueId,
    ) -> ValueId {
        if matches!(index, Expr::Number { value, .. } if *value == 0.0) {
            self.push_call("firstOf", vec![array])
        } else {
            self.push_call("valueInArray", vec![array, index_value])
        }
    }

    fn replace_array_element(
        &mut self,
        array: ValueId,
        index: ValueId,
        replacement: ValueId,
    ) -> ValueId {
        let zero = self.push_number(0.0);
        let one = self.push_number(1.0);
        let end = self.push_call("add", vec![index, one]);
        let maximum = self.push_number(999_999_999_999.0);
        let prefix = self.push_call("slice", vec![array, zero, index]);
        let middle = self.lower_array(vec![replacement]);
        let suffix = self.push_call("slice", vec![array, end, maximum]);
        let with_replacement = self.push_call("appendToArray", vec![prefix, middle]);
        self.push_call("appendToArray", vec![with_replacement, suffix])
    }

    fn rebuild_deleted_array(
        &mut self,
        array: ValueId,
        indices: &[&Expr],
    ) -> Result<ValueId, IntegrationError> {
        let index = self.lower_value(indices[0])?;
        if indices.len() == 1 {
            let current_index = self.push_call("currentArrayIndex", Vec::new());
            let condition = self.push_call("!=", vec![current_index, index]);
            return Ok(self.push_call("filteredArray", vec![array, condition]));
        }
        let child = self.lower_indexed_read(array, indices[0], index);
        let replacement = self.rebuild_deleted_array(child, &indices[1..])?;
        Ok(self.replace_array_element_for_delete(array, indices[0], index, replacement))
    }

    fn replace_array_element_for_delete(
        &mut self,
        array: ValueId,
        index_expr: &Expr,
        index: ValueId,
        replacement: ValueId,
    ) -> ValueId {
        if let Some(number) = hir::visit::literal_number(index_expr) {
            let middle = self.lower_array(vec![replacement]);
            let maximum = self.push_number(999_999_999_999.0);
            let suffix_start = self.push_number(number + 1.0);
            let suffix = self.push_call("slice", vec![array, suffix_start, maximum]);
            if number == 0.0 {
                return self.push_call("appendToArray", vec![middle, suffix]);
            }
            let zero = self.push_number(0.0);
            let prefix = self.push_call("slice", vec![array, zero, index]);
            let with_replacement = self.push_call("appendToArray", vec![prefix, middle]);
            return self.push_call("appendToArray", vec![with_replacement, suffix]);
        }
        self.replace_array_element(array, index, replacement)
    }
}
