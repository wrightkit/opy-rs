//! Backend preparation shared by source compilation and settings resolution.

use super::*;

impl Compiler {
    pub(crate) fn compile_hir_with_locale_and_hook(
        &self,
        hir: &hir::Program,
        hook: Option<crate::PostCompileHookRecord>,
        locale: &Locale,
    ) -> Result<CompilationArtifact, IntegrationError> {
        let mut artifact = self.compile_hir_with_locale(hir, locale)?;
        if let Some(hook) = hook {
            let runtime = crate::macro_js::MacroRuntime::new(crate::macro_js::Limits::default());
            let result = runtime
                .run_hook(&hook.source, &artifact.emitted, &hook.script)
                .map_err(|error| {
                    IntegrationError::post_compile_hook(error, hook.span.map(hir_span_from_diag))
                })?;
            artifact.final_output = result.text;
            artifact.hook_console_output = result.console_output;
        }
        Ok(artifact)
    }
}

pub(crate) fn reject_unlowered_directives(hir: &hir::Program) -> Result<(), IntegrationError> {
    if let Some(replacement) = hir.preprocessing.replacements.first() {
        let span = hir
            .preprocessing
            .directives
            .iter()
            .find(|directive| directive.name.starts_with("replace"))
            .and_then(|directive| directive.span)
            .or(replacement.span);
        return Err(IntegrationError::new(
            "backend-directive-unsupported",
            format!(
                "replacement directive '{}' has no canonical workshop-rs lowering",
                replacement.value
            ),
            span,
        ));
    }
    if let Some(replacement) = hir
        .preprocessing
        .directives
        .iter()
        .find(|directive| directive.name.starts_with("replace"))
    {
        return Err(IntegrationError::new(
            "backend-directive-unsupported",
            format!(
                "replacement directive '{}' has no canonical workshop-rs lowering",
                replacement.name
            ),
            replacement.span,
        ));
    }
    Ok(())
}

pub(crate) type MacroBindings = HashMap<String, Expr>;

pub(crate) struct MacroExpander {
    macros: HashMap<String, (Vec<String>, Vec<Stmt>)>,
    stack: Vec<String>,
}

impl MacroExpander {
    pub(crate) fn from_program(program: &hir::Program) -> Self {
        let macros = program
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::Macro {
                    name, args, body, ..
                } => Some((name.clone(), (args.clone(), body.clone()))),
                _ => None,
            })
            .collect();
        Self {
            macros,
            stack: Vec::new(),
        }
    }
}

pub(crate) fn expand_macros(program: &hir::Program) -> Result<hir::Program, IntegrationError> {
    let mut expander = MacroExpander::from_program(program);
    let mut expanded = program.clone();
    let bindings = MacroBindings::new();

    for declaration in &mut expanded.declarations {
        match declaration {
            hir::Declaration::GlobalVariable { initializer, .. }
            | hir::Declaration::PlayerVariable { initializer, .. } => {
                if let Some(initializer) = initializer {
                    **initializer = expander.expand_expr(initializer, &bindings)?;
                }
            }
            _ => {}
        }
    }
    for entry in &mut expanded.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for argument in &mut rule.event.args {
                    *argument = expander.expand_expr(argument, &bindings)?;
                }
                for condition in &mut rule.conditions {
                    *condition = expander.expand_expr(condition, &bindings)?;
                }
                rule.actions = expander.expand_stmts(&rule.actions, &bindings)?;
            }
            RuleEntry::SubroutineDef { body, .. } => {
                *body = expander.expand_stmts(body, &bindings)?;
            }
        }
    }
    Ok(expanded)
}

impl MacroExpander {
    fn expand_stmts(
        &mut self,
        statements: &[Stmt],
        bindings: &MacroBindings,
    ) -> Result<Vec<Stmt>, IntegrationError> {
        let mut expanded = Vec::new();
        for statement in statements {
            if let Stmt::Expr { expr, .. } = statement {
                if let Expr::MacroCall { name, args, span } = expr.as_ref() {
                    let args = args
                        .iter()
                        .map(|arg| self.expand_expr(arg, bindings))
                        .collect::<Result<Vec<_>, _>>()?;
                    expanded.extend(self.expand_macro_body(name, &args, *span)?);
                    continue;
                }
            }
            expanded.push(self.expand_stmt(statement, bindings)?);
        }
        Ok(expanded)
    }

    fn expand_stmt(
        &mut self,
        statement: &Stmt,
        bindings: &MacroBindings,
    ) -> Result<Stmt, IntegrationError> {
        Ok(match statement {
            Stmt::Expr { expr, span } => Stmt::Expr {
                expr: Box::new(self.expand_expr(expr, bindings)?),
                span: *span,
            },
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: Box::new(self.expand_expr(target, bindings)?),
                value: Box::new(self.expand_expr(value, bindings)?),
                span: *span,
            },
            Stmt::If {
                branches,
                r#else,
                span,
            } => Stmt::If {
                branches: branches
                    .iter()
                    .map(|branch| {
                        Ok(hir::types::IfBranch {
                            condition: Box::new(self.expand_expr(&branch.condition, bindings)?),
                            body: self.expand_stmts(&branch.body, bindings)?,
                        })
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?,
                r#else: r#else
                    .as_ref()
                    .map(|body| self.expand_stmts(body, bindings))
                    .transpose()?,
                span: *span,
            },
            Stmt::For {
                variable,
                iterable,
                body,
                span,
            } => Stmt::For {
                variable: Box::new(self.expand_expr(variable, bindings)?),
                iterable: Box::new(self.expand_expr(iterable, bindings)?),
                body: self.expand_stmts(body, bindings)?,
                span: *span,
            },
            Stmt::While {
                condition,
                body,
                span,
            } => Stmt::While {
                condition: Box::new(self.expand_expr(condition, bindings)?),
                body: self.expand_stmts(body, bindings)?,
                span: *span,
            },
            Stmt::DoWhile {
                condition,
                body,
                span,
            } => Stmt::DoWhile {
                condition: Box::new(self.expand_expr(condition, bindings)?),
                body: self.expand_stmts(body, bindings)?,
                span: *span,
            },
            Stmt::Switch { value, arms, span } => Stmt::Switch {
                value: Box::new(self.expand_expr(value, bindings)?),
                arms: arms
                    .iter()
                    .map(|arm| match arm {
                        SwitchArm::Case { value, body, span } => Ok(SwitchArm::Case {
                            value: Box::new(self.expand_expr(value, bindings)?),
                            body: self.expand_stmts(body, bindings)?,
                            span: *span,
                        }),
                        SwitchArm::Default { body, span } => Ok(SwitchArm::Default {
                            body: self.expand_stmts(body, bindings)?,
                            span: *span,
                        }),
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?,
                span: *span,
            },
            Stmt::Delete { target, span } => Stmt::Delete {
                target: Box::new(self.expand_expr(target, bindings)?),
                span: *span,
            },
            Stmt::Goto {
                label,
                offset,
                rule_start,
                span,
            } => Stmt::Goto {
                label: label.clone(),
                offset: offset
                    .as_ref()
                    .map(|offset| self.expand_expr(offset, bindings).map(Box::new))
                    .transpose()?,
                rule_start: *rule_start,
                span: *span,
            },
            Stmt::Break { .. }
            | Stmt::Return { .. }
            | Stmt::CallSubroutine { .. }
            | Stmt::Pass { .. } => statement.clone(),
            Stmt::Continue { .. } | Stmt::Label { .. } => statement.clone(),
        })
    }

    pub(crate) fn expand_expr(
        &mut self,
        expression: &Expr,
        bindings: &MacroBindings,
    ) -> Result<Expr, IntegrationError> {
        match expression {
            Expr::MacroParam { name, span } => bindings.get(name).cloned().ok_or_else(|| {
                IntegrationError::new(
                    "unsupported-integration-surface",
                    format!("macro parameter '{name}' has no expansion binding"),
                    *span,
                )
            }),
            Expr::MacroCall { name, args, span } => {
                let args = args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?;
                let body = self.expand_macro_body(name, &args, *span)?;
                if body.len() != 1 {
                    return Err(IntegrationError::new(
                        "macro-invalid",
                        format!("macro '{name}' must produce one expression in value position"),
                        *span,
                    ));
                }
                match body.into_iter().next().expect("one macro body statement") {
                    Stmt::Expr { expr, .. } => Ok(*expr),
                    _ => Err(IntegrationError::new(
                        "macro-invalid",
                        format!("macro '{name}' must produce an expression in value position"),
                        *span,
                    )),
                }
            }
            Expr::Array { elements, span } => Ok(Expr::Array {
                elements: elements
                    .iter()
                    .map(|element| self.expand_expr(element, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::Dict { entries, span } => Ok(Expr::Dict {
                entries: entries
                    .iter()
                    .map(|entry| {
                        Ok(hir::DictEntry {
                            key: Box::new(self.expand_expr(&entry.key, bindings)?),
                            value: Box::new(self.expand_expr(&entry.value, bindings)?),
                            span: entry.span,
                        })
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?,
                span: *span,
            }),
            Expr::Comprehension {
                element,
                variable,
                variable_span,
                index,
                index_span,
                iterable,
                condition,
                span,
            } => Ok(Expr::Comprehension {
                element: Box::new(self.expand_expr(element, bindings)?),
                variable: variable.clone(),
                variable_span: *variable_span,
                index: index.clone(),
                index_span: *index_span,
                iterable: Box::new(self.expand_expr(iterable, bindings)?),
                condition: condition
                    .as_ref()
                    .map(|condition| self.expand_expr(condition, bindings).map(Box::new))
                    .transpose()?,
                span: *span,
            }),
            Expr::Lambda {
                params,
                param_spans,
                body,
                span,
            } => Ok(Expr::Lambda {
                params: params.clone(),
                param_spans: param_spans.clone(),
                body: Box::new(self.expand_expr(body, bindings)?),
                span: *span,
            }),
            Expr::Type { name, args, span } => Ok(Expr::Type {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::Vector { x, y, z, span } => Ok(Expr::Vector {
                x: Box::new(self.expand_expr(x, bindings)?),
                y: Box::new(self.expand_expr(y, bindings)?),
                z: Box::new(self.expand_expr(z, bindings)?),
                span: *span,
            }),
            Expr::PlayerVar {
                player,
                name,
                member_span,
                span,
            } => Ok(Expr::PlayerVar {
                player: Box::new(self.expand_expr(player, bindings)?),
                name: name.clone(),
                member_span: *member_span,
                span: *span,
            }),
            Expr::Member {
                receiver,
                member,
                member_span,
                span,
            } => Ok(Expr::Member {
                receiver: Box::new(self.expand_expr(receiver, bindings)?),
                member: member.clone(),
                member_span: *member_span,
                span: *span,
            }),
            Expr::Call { name, args, span } => Ok(Expr::Call {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                span,
            } => Ok(Expr::ReceiverCall {
                receiver: Box::new(self.expand_expr(receiver, bindings)?),
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => Ok(Expr::Binary {
                op: op.clone(),
                left: Box::new(self.expand_expr(left, bindings)?),
                right: Box::new(self.expand_expr(right, bindings)?),
                span: *span,
            }),
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                span,
            } => Ok(Expr::Conditional {
                then_value: Box::new(self.expand_expr(then_value, bindings)?),
                condition: Box::new(self.expand_expr(condition, bindings)?),
                else_value: Box::new(self.expand_expr(else_value, bindings)?),
                span: *span,
            }),
            Expr::Unary { op, operand, span } => Ok(Expr::Unary {
                op: op.clone(),
                operand: Box::new(self.expand_expr(operand, bindings)?),
                span: *span,
            }),
            Expr::Index { array, index, span } => Ok(Expr::Index {
                array: Box::new(self.expand_expr(array, bindings)?),
                index: Box::new(self.expand_expr(index, bindings)?),
                span: *span,
            }),
            Expr::Format { text, args, span } => Ok(Expr::Format {
                text: text.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            _ => Ok(expression.clone()),
        }
    }

    fn expand_macro_body(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<Vec<Stmt>, IntegrationError> {
        let Some((params, body)) = self.macros.get(name).cloned() else {
            return Err(IntegrationError::new(
                "unsupported-integration-surface",
                format!("macro '{name}' has no declaration"),
                span,
            ));
        };
        if params.len() != args.len() {
            return Err(IntegrationError::new(
                "macro-arity",
                format!(
                    "macro '{name}' expects {} argument(s) but got {}",
                    params.len(),
                    args.len()
                ),
                span,
            ));
        }
        if self.stack.iter().any(|active| active == name) {
            return Err(IntegrationError::new(
                "macro-recursion",
                format!("recursive macro expansion detected for '{name}'"),
                span,
            ));
        }
        let mut bindings = MacroBindings::new();
        for (param, arg) in params.into_iter().zip(args.iter()) {
            bindings.insert(param, arg.clone());
        }
        self.stack.push(name.to_string());
        let result = self.expand_stmts(&body, &bindings);
        self.stack.pop();
        result
    }
}
