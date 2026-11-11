use super::*;

impl Lowering<'_> {
    pub(super) fn lower_switch(
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
        let mut has_default = false;
        let mut case_offsets = Vec::new();
        let mut default_offset = None;
        let mut offset = 0;
        let mut switch_body = Vec::new();

        let mut reverse_bodies =
            (loop_continue.is_some() && !do_while_continue).then(|| vec![None; arms.len()]);
        if let Some((outer_after, structural_after)) = loop_continue {
            let mut future = Vec::new();
            // Break markers stay out of `future`: an unmatched `Else` has no
            // canonical width, so later markers are counted structurally.
            let mut later_breaks = 0;
            for index in (0..arms.len()).rev() {
                let body = match &arms[index] {
                    SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => body,
                };
                let mut after = future.clone();
                after.extend_from_slice(outer_after);
                let lowered = self.lower_switch_body(
                    body,
                    Some((&after, structural_after + later_breaks)),
                    do_while_continue,
                )?;
                later_breaks += usize::from(lowered.1.is_some());
                let mut next_future = lowered.0.clone();
                next_future.extend_from_slice(&future);
                future = next_future;
                reverse_bodies.as_mut().unwrap()[index] = Some(lowered);
            }
        }
        for (index, arm) in arms.iter().enumerate() {
            let (is_case, (body, switch_break)) = match arm {
                SwitchArm::Case { value, body, .. } => {
                    case_values.push(self.lower_value(value)?);
                    let lowered = if let Some(bodies) = reverse_bodies.as_mut() {
                        bodies[index].take().unwrap()
                    } else {
                        self.lower_switch_body(body, loop_continue, do_while_continue)?
                    };
                    (true, lowered)
                }
                SwitchArm::Default { body, span } => {
                    if has_default {
                        return Err(
                            self.unsupported("a switch may contain at most one default arm", *span)
                        );
                    }
                    has_default = true;
                    default_offset = Some(offset);
                    let lowered = if let Some(bodies) = reverse_bodies.as_mut() {
                        bodies[index].take().unwrap()
                    } else {
                        self.lower_switch_body(body, loop_continue, do_while_continue)?
                    };
                    (false, lowered)
                }
            };
            if is_case {
                case_offsets.push(offset);
            }
            offset +=
                self.canonical_action_width(&body, span)? + usize::from(switch_break.is_some());
            match switch_break {
                Some((at, marker)) => {
                    switch_body.extend_from_slice(&body[..at]);
                    switch_body.push(marker);
                    switch_body.extend_from_slice(&body[at..]);
                }
                None => switch_body.extend(body),
            }
        }

        let case_values = self.lower_array(case_values);
        let offset_values = std::iter::once(default_offset.unwrap_or(offset))
            .chain(case_offsets)
            .map(|value| self.push_number(value as f64))
            .collect();
        let offsets = self.lower_array(offset_values);
        let skip = self.lower_switch_selector(selector, case_values, offsets);
        switch_body.insert(0, skip);
        let true_value = self.push_value(Value::Bool(true));
        Ok(self.push_if_actions(vec![(true_value, switch_body)], None))
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

    fn lower_switch_body(
        &mut self,
        statements: &[Stmt],
        loop_continue: Option<(&[ActionId], usize)>,
        do_while_continue: bool,
    ) -> Result<LoweredSwitchBody, IntegrationError> {
        let break_index = statements
            .iter()
            .position(|statement| matches!(statement, Stmt::Break { .. }));
        let body_end = break_index.unwrap_or(statements.len());
        let mut marker = None;
        let mut tail = Vec::new();
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
            let [break_marker] =
                self.lower_action(&statements[index], Some(BreakTarget::Switch))?[..]
            else {
                unreachable!("a switch break must lower to one Else marker")
            };
            marker = Some(break_marker);
            for statement in &statements[index + 1..] {
                tail.extend(self.lower_action(statement, Some(BreakTarget::Switch))?);
            }
        }
        let mut actions = Vec::new();
        if do_while_continue {
            actions.extend(self.lower_do_while_body(&statements[..body_end])?);
        } else if let Some((after, structural_after)) = loop_continue {
            let mut continuation = tail.clone();
            continuation.extend_from_slice(after);
            actions.extend(self.lower_loop_sequence_with_break_target(
                &statements[..body_end],
                &continuation,
                structural_after + 1 + usize::from(marker.is_some()),
                BreakTarget::Switch,
            )?);
        } else {
            for statement in &statements[..body_end] {
                actions.extend(self.lower_action(statement, Some(BreakTarget::Switch))?);
            }
        }
        let break_at = marker.map(|marker| (actions.len(), marker));
        actions.extend(tail);
        Ok((actions, break_at))
    }
}
