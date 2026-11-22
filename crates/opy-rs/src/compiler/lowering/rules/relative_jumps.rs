use super::*;

struct RelativeJumpPasses {
    after_jump: bool,
    preserved: HashSet<*const Stmt>,
}

impl hir::visit::Visitor for RelativeJumpPasses {
    fn visit_expr(&mut self, _: &Expr) {}

    fn visit_stmt(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Goto {
                offset: Some(_), ..
            } => self.after_jump = true,
            Stmt::Expr { expr, .. } if matches!(expr.as_ref(), Expr::Call { name, .. } if name == "__skip__" || name == "__skipIf__") =>
            {
                self.after_jump = true;
            }
            _ => {}
        }
        hir::visit::walk_stmt(self, statement);
        let preserve = match statement {
            Stmt::Pass { .. } | Stmt::If { .. } | Stmt::While { .. } => true,
            Stmt::Expr { expr, .. } => {
                matches!(expr.as_ref(), Expr::Call { name, args, .. } if name == "pass" && args.is_empty())
            }
            _ => false,
        };
        if self.after_jump && preserve {
            self.preserved.insert(std::ptr::from_ref(statement));
        }
    }
}

impl Lowering<'_> {
    pub(super) fn prepare_relative_jump_passes(
        &mut self,
        statements: &[Stmt],
        span: Option<HirSpan>,
    ) {
        let mut visitor = RelativeJumpPasses {
            after_jump: !self.optimization_state_at(span.as_ref()).enabled,
            preserved: HashSet::new(),
        };
        hir::visit::walk_stmts(&mut visitor, statements);
        self.relative_jump_preserved = visitor.preserved;
    }

    pub(super) fn preserves_relative_jump_statement(&self, statement: &Stmt) -> bool {
        self.relative_jump_preserved
            .contains(&std::ptr::from_ref(statement))
    }

    pub(super) fn lower_pass(&mut self, statement: &Stmt) -> Vec<ActionId> {
        if self.preserves_relative_jump_statement(statement) {
            vec![self.push_call_action("disabledAbort", &[])]
        } else {
            Vec::new()
        }
    }

    pub(super) fn lower_skip_with_distance(
        &mut self,
        conditions: &[&Expr],
        distance: usize,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let mut args = Vec::with_capacity(conditions.len() + 1);
        if let Some(condition) = self.lower_condition_chain(conditions)? {
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
        self.mark_action_origins(std::slice::from_ref(&skip), span);
        Ok(skip)
    }

    pub(super) fn has_preserved_pass_in_if(&self, statement: &Stmt) -> bool {
        let Stmt::If { branches, .. } = statement else {
            return false;
        };
        self.preserves_relative_jump_statement(statement)
            && branches.iter().any(|branch| {
                branch
                    .body
                    .iter()
                    .any(|stmt| matches!(stmt, Stmt::Pass { .. }))
            })
    }

    pub(super) fn lower_aggressive_relative_pass(
        &mut self,
        statement: &Stmt,
        followed: bool,
        break_target: Option<BreakTarget>,
    ) -> Result<Option<Vec<ActionId>>, IntegrationError> {
        let Stmt::If {
            branches,
            r#else: None,
            span,
            ..
        } = statement
        else {
            return Ok(None);
        };
        let [branch] = branches.as_slice() else {
            return Ok(None);
        };
        let single_pass = matches!(branch.body.as_slice(), [Stmt::Pass { .. }]);
        if !self.has_preserved_pass_in_if(statement) {
            return Ok(None);
        }
        let optimization = self.optimization_state_at(statement.span());
        if !optimization.enabled || !optimization.for_size || !optimization.for_size_aggressive {
            return Ok(None);
        }
        let condition = self.lower_value(&branch.condition)?;
        let condition = self.materialize_value(condition);
        let include_end = followed || break_target.is_some();
        if !matches!(condition, workshop_rs::Value::Call { ref name, .. }
            if name == "not" || (single_pass && include_end && matches!(name.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=")))
        {
            return Ok(None);
        }
        let condition = if let Some(operand) = tail_negation::operand(self, &branch.condition)? {
            self.lower_value(operand)?
        } else {
            let negated = Expr::Unary {
                op: "not".to_string(),
                operand: branch.condition.clone(),
                span: *span,
            };
            self.lower_value(&negated)?
        };
        let body = self.lower_actions(&branch.body, break_target, followed)?;
        let distance = self.canonical_action_width(&body, *span)?;
        let distance = self.push_number(distance as f64);
        let skip = self.push_call_action("skipIf", &[condition, distance]);
        self.mark_action_origins(std::slice::from_ref(&skip), *span);
        let mut actions = vec![skip];
        actions.extend(body);
        actions.push(self.push_call_action("disabledAbort", &[]));
        Ok(Some(actions))
    }

    pub(super) fn has_meaningful_rule_action(&self, actions: &[ActionId], event: &Event) -> bool {
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
                    "abort" | "abortIf" | "break" | "continue" | "disabledAbort" | "loop"
                    | "loopIf" | "return" | "skip" | "skipIf" => false,
                    "wait" => matches!(event, Event::Subroutine(_)),
                    _ => true,
                },
                Some(_) => true,
            })
    }
}

pub(super) fn switch_body_is_noop(statements: &[Stmt], preserved: &HashSet<*const Stmt>) -> bool {
    statements.iter().all(|statement| match statement {
        Stmt::Pass { .. } => !preserved.contains(&std::ptr::from_ref(statement)),
        Stmt::Break { .. } => true,
        Stmt::If {
            branches, r#else, ..
        } => {
            branches
                .iter()
                .all(|branch| switch_body_is_noop(&branch.body, preserved))
                && r#else
                    .as_ref()
                    .is_none_or(|body| switch_body_is_noop(body, preserved))
        }
        Stmt::Switch { arms, .. } => arms.iter().all(|arm| match arm {
            SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => {
                switch_body_is_noop(body, preserved)
            }
        }),
        _ => false,
    })
}
