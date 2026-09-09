use super::*;

impl Lowerer {
    pub(super) fn lower_rule(
        &mut self,
        rule: &cst::Rule,
        files: &[SourceFile],
        preprocessing: &PreprocessingState,
    ) -> OpyResult<Rule> {
        if let Some(annotation) = rule
            .annotations
            .iter()
            .find(|annotation| annotation.name == "Name")
        {
            self.error_at(
                "duplicate-rule-name",
                "Rule name was already declared".to_string(),
                annotation.span,
            );
        }
        let conditions = rule
            .conditions
            .iter()
            .map(|condition| self.lower_expr(condition, &[], CallPosition::Value))
            .collect();
        let actions = self.lower_block(&rule.actions, &[], false, true, false);
        Ok(Rule {
            name: render_rule_name(
                &rule.name,
                rule.rule_prefix.as_deref(),
                rule.delimiter,
                rule.span,
                files,
                preprocessing,
            )?,
            span: Some(rule.span.into()),
            name_span: Some(rule.name_span.into()),
            disabled: rule.disabled,
            delimiter: rule.delimiter,
            new_page: rule.new_page.clone(),
            annotations: lower_annotations(&rule.annotations),
            event: Event {
                name: rule.event.name.clone(),
                args: rule
                    .event
                    .args
                    .iter()
                    .map(|arg| self.lower_expr(arg, &[], CallPosition::Value))
                    .collect(),
                span: Some(rule.event.span.into()),
            },
            conditions,
            actions,
        })
    }

    pub(super) fn lower_block(
        &mut self,
        stmts: &[Stmt],
        macro_params: &[String],
        breakable: bool,
        allow_do_while: bool,
        loopable: bool,
    ) -> Vec<HirStmt> {
        let mut lowered = Vec::new();
        for (index, stmt) in stmts.iter().enumerate() {
            if matches!(stmt, Stmt::DoWhile { .. })
                && (!allow_do_while
                    || stmts[..index]
                        .iter()
                        .any(|previous| !matches!(previous, Stmt::Pass { .. })))
            {
                self.error_at(
                    "do-while-placement",
                    "do-while must be at the beginning of a rule, subroutine, or do-while body; only pass statements may precede it".to_string(),
                    stmt.span(),
                );
            }
            if let Stmt::Expr {
                expr: Expr::Call { name, args, .. },
                span,
            } = stmt
            {
                if name == "splitDictArray" {
                    lowered.extend(self.lower_split_dict_array(args, *span, macro_params));
                    continue;
                }
            }
            lowered.push(self.lower_stmt(stmt, macro_params, breakable, loopable));
        }
        lowered
    }

    pub(super) fn lower_stmt(
        &mut self,
        stmt: &Stmt,
        macro_params: &[String],
        breakable: bool,
        loopable: bool,
    ) -> HirStmt {
        match stmt {
            Stmt::Expr { expr, span } => {
                // A bare call of a declared subroutine becomes
                // `CallSubroutine` (reference behavior).
                if let Expr::Call { name, args, .. } = expr {
                    if self.subroutine_visible(name) && args.is_empty() {
                        return HirStmt::CallSubroutine {
                            name: name.clone(),
                            span: Some(span.into()),
                        };
                    }
                }
                HirStmt::Expr {
                    expr: Box::new(self.lower_expr(expr, macro_params, CallPosition::Statement)),
                    span: Some(span.into()),
                }
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                if indexed_expr_depth(target) >= 4 {
                    self.error_at(
                        "four-dimensional-assignment",
                        "Cannot assign to 4d array".to_string(),
                        target.span(),
                    );
                }
                HirStmt::Assign {
                    target: Box::new(self.lower_expr(target, macro_params, CallPosition::Value)),
                    value: Box::new(self.lower_expr(value, macro_params, CallPosition::Value)),
                    span: Some(span.into()),
                }
            }
            Stmt::If {
                branches,
                r#else,
                span,
            } => HirStmt::If {
                branches: branches
                    .iter()
                    .map(|branch| IfBranch {
                        condition: Box::new(self.lower_expr(
                            &branch.condition,
                            macro_params,
                            CallPosition::Value,
                        )),
                        body: self.lower_block(
                            &branch.body,
                            macro_params,
                            breakable,
                            false,
                            loopable,
                        ),
                    })
                    .collect(),
                r#else: r#else
                    .as_ref()
                    .map(|body| self.lower_block(body, macro_params, breakable, false, loopable)),
                span: Some(span.into()),
            },
            Stmt::For {
                variable,
                iterable,
                body,
                span,
            } => {
                // The reference accepts only `range(...)` as a `for ... in`
                // iterable; other iterables are an explicit frontend error
                // (recovered by lowering in value position).
                let iterable_position = if matches!(iterable, Expr::Call { name, .. } if name == "range")
                {
                    CallPosition::ForIterable
                } else {
                    self.error_at(
                        "invalid-iterable",
                        "for-loop iterable must be a range(...) call".to_string(),
                        iterable.span(),
                    );
                    CallPosition::Value
                };
                HirStmt::For {
                    variable: Box::new(self.lower_for_binder(variable, macro_params)),
                    iterable: Box::new(self.lower_expr(iterable, macro_params, iterable_position)),
                    body: self.lower_block(body, macro_params, true, false, true),
                    span: Some(span.into()),
                }
            }
            Stmt::While {
                condition,
                body,
                span,
            } => HirStmt::While {
                condition: Box::new(self.lower_expr(condition, macro_params, CallPosition::Value)),
                body: self.lower_block(body, macro_params, true, false, true),
                span: Some(span.into()),
            },
            Stmt::DoWhile {
                condition,
                body,
                span,
            } => HirStmt::DoWhile {
                condition: Box::new(self.lower_expr(condition, macro_params, CallPosition::Value)),
                body: self.lower_block(body, macro_params, true, true, true),
                span: Some(span.into()),
            },
            Stmt::Switch { value, arms, span } => HirStmt::Switch {
                value: Box::new(self.lower_expr(value, macro_params, CallPosition::Value)),
                arms: arms
                    .iter()
                    .map(|arm| match arm {
                        cst::SwitchArm::Case { value, body, span } => HirSwitchArm::Case {
                            value: Box::new(self.lower_expr(
                                value,
                                macro_params,
                                CallPosition::Value,
                            )),
                            body: self.lower_block(body, macro_params, true, false, loopable),
                            span: Some((*span).into()),
                        },
                        cst::SwitchArm::Default { body, span } => HirSwitchArm::Default {
                            body: self.lower_block(body, macro_params, true, false, loopable),
                            span: Some((*span).into()),
                        },
                    })
                    .collect(),
                span: Some(span.into()),
            },
            Stmt::Delete { target, span } => HirStmt::Delete {
                target: Box::new(self.lower_expr(target, macro_params, CallPosition::Value)),
                span: Some(span.into()),
            },
            Stmt::Break { span } => {
                if !breakable {
                    self.error_at(
                        "break-context",
                        "break is only valid inside a switch or loop".to_string(),
                        *span,
                    );
                }
                HirStmt::Break {
                    span: Some(span.into()),
                }
            }
            Stmt::Return { span } => HirStmt::Return {
                span: Some(span.into()),
            },
            Stmt::Continue { span } => {
                if !loopable {
                    self.error_at(
                        "continue-context",
                        "continue is only valid inside a loop".to_string(),
                        *span,
                    );
                }
                HirStmt::Continue {
                    span: Some(span.into()),
                }
            }
            Stmt::Goto {
                label,
                offset,
                rule_start,
                span,
            } => HirStmt::Goto {
                label: label.clone(),
                offset: offset.as_ref().map(|offset| {
                    Box::new(self.lower_expr(offset, macro_params, CallPosition::Value))
                }),
                rule_start: *rule_start,
                span: Some(span.into()),
            },
            Stmt::Label { name, span } => HirStmt::Label {
                name: name.clone(),
                span: Some(span.into()),
            },
            Stmt::Pass { span } => HirStmt::Pass {
                span: Some(span.into()),
            },
        }
    }

    pub(super) fn lower_for_binder(&mut self, variable: &Expr, macro_params: &[String]) -> HirExpr {
        if let Expr::Member {
            receiver,
            member,
            member_span,
            span,
        } = variable
        {
            if !default_var_index(member).is_some() && !self.player_visible(member) {
                self.error_at(
                    "unknown-identifier",
                    format!("unknown player variable '{member}'"),
                    *member_span,
                );
                return HirExpr::Null { span: None };
            }
            return HirExpr::PlayerVar {
                player: Box::new(self.lower_expr(receiver, macro_params, CallPosition::Value)),
                name: member.clone(),
                member_span: Some((*member_span).into()),
                span: Some((*span).into()),
            };
        }
        let lowered = self.lower_expr(variable, macro_params, CallPosition::Value);
        if !matches!(
            lowered,
            HirExpr::GlobalVar { .. } | HirExpr::PlayerVar { .. }
        ) {
            self.error_at(
                "invalid-range-binder",
                format!(
                    "Expected variable for 1st argument of function 'for', but got {}",
                    lowered.kind_name()
                ),
                variable.span(),
            );
        }
        lowered
    }
}
