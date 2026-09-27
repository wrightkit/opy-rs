use super::*;

pub(crate) fn reject_unlowered_directives(hir: &hir::Program) -> Result<(), IntegrationError> {
    let unsupported = [
        "disableTranslationSourceLines",
        "keepUnusedTranslations",
        "writeToOutputFile",
    ];
    if let Some(directive) = hir
        .preprocessing
        .directives
        .iter()
        .find(|directive| unsupported.contains(&directive.name.as_str()))
    {
        return Err(IntegrationError::new(
            "backend-directive-unsupported",
            format!(
                "directive '{}' requires an editor or translation-output capability outside the forward compiler contract",
                directive.name
            ),
            directive.span,
        ));
    }
    Ok(())
}

pub(crate) type MacroBindings = HashMap<String, Expr>;

pub(crate) struct MacroExpander {
    macros: HashMap<String, (Vec<String>, Vec<Stmt>)>,
    stack: Vec<String>,
    /// Relocate expanded spans to the invocation site; only the source
    /// attribution pass sets it, so diagnostics keep the definition spans.
    attribute_to_site: bool,
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
            attribute_to_site: false,
        }
    }
}

pub(crate) fn expand_macros(program: &hir::Program) -> Result<hir::Program, IntegrationError> {
    expand_macros_with(program, false)
}

/// Expand macros with every expanded span attributed to its invocation site.
pub(crate) fn expand_macros_attributed(
    program: &hir::Program,
) -> Result<hir::Program, IntegrationError> {
    expand_macros_with(program, true)
}

fn expand_macros_with(
    program: &hir::Program,
    attribute_to_site: bool,
) -> Result<hir::Program, IntegrationError> {
    let mut expander = MacroExpander::from_program(program);
    expander.attribute_to_site = attribute_to_site;
    let mut expanded = program.clone();
    let bindings = MacroBindings::new();

    for declaration in &mut expanded.declarations {
        match declaration {
            hir::Declaration::GlobalVariable { initializer, .. }
            | hir::Declaration::PlayerVariable { initializer, .. } => {
                if let Some(initializer) = initializer {
                    **initializer = expander.expand_expr(initializer, &bindings, None)?;
                }
            }
            _ => {}
        }
    }
    for entry in &mut expanded.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for argument in &mut rule.event.args {
                    *argument = expander.expand_expr(argument, &bindings, None)?;
                }
                for condition in &mut rule.conditions {
                    *condition = expander.expand_expr(condition, &bindings, None)?;
                }
                rule.actions = expander.expand_stmts(&rule.actions, &bindings, None)?;
            }
            RuleEntry::SubroutineDef { body, .. } => {
                *body = expander.expand_stmts(body, &bindings, None)?;
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
        site: Option<HirSpan>,
    ) -> Result<Vec<Stmt>, IntegrationError> {
        let mut expanded = Vec::new();
        for statement in statements {
            if let Stmt::Expr { expr, .. } = statement {
                if let Expr::MacroCall { name, args, span } = expr.as_ref() {
                    let args = args
                        .iter()
                        .map(|arg| self.expand_expr(arg, bindings, site))
                        .collect::<Result<Vec<_>, _>>()?;
                    expanded.extend(self.expand_macro_body(name, &args, *span, site)?);
                    continue;
                }
            }
            expanded.push(self.expand_stmt(statement, bindings, site)?);
        }
        Ok(expanded)
    }

    fn expand_stmt(
        &mut self,
        statement: &Stmt,
        bindings: &MacroBindings,
        site: Option<HirSpan>,
    ) -> Result<Stmt, IntegrationError> {
        let mut expanded = self.expand_stmt_inner(statement, bindings, site)?;
        if let Some(site) = site {
            set_stmt_span(&mut expanded, site);
        }
        Ok(expanded)
    }

    fn expand_stmt_inner(
        &mut self,
        statement: &Stmt,
        bindings: &MacroBindings,
        site: Option<HirSpan>,
    ) -> Result<Stmt, IntegrationError> {
        Ok(match statement {
            Stmt::Expr { expr, span } => Stmt::Expr {
                expr: Box::new(self.expand_expr(expr, bindings, site)?),
                span: *span,
            },
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: Box::new(self.expand_expr(target, bindings, site)?),
                value: Box::new(self.expand_expr(value, bindings, site)?),
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
                            condition: Box::new(self.expand_expr(
                                &branch.condition,
                                bindings,
                                site,
                            )?),
                            body: self.expand_stmts(&branch.body, bindings, site)?,
                        })
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?,
                r#else: r#else
                    .as_ref()
                    .map(|body| self.expand_stmts(body, bindings, site))
                    .transpose()?,
                span: *span,
            },
            Stmt::For {
                variable,
                iterable,
                body,
                span,
            } => Stmt::For {
                variable: Box::new(self.expand_expr(variable, bindings, site)?),
                iterable: Box::new(self.expand_expr(iterable, bindings, site)?),
                body: self.expand_stmts(body, bindings, site)?,
                span: *span,
            },
            Stmt::While {
                condition,
                body,
                span,
            } => Stmt::While {
                condition: Box::new(self.expand_expr(condition, bindings, site)?),
                body: self.expand_stmts(body, bindings, site)?,
                span: *span,
            },
            Stmt::DoWhile {
                condition,
                body,
                span,
            } => Stmt::DoWhile {
                condition: Box::new(self.expand_expr(condition, bindings, site)?),
                body: self.expand_stmts(body, bindings, site)?,
                span: *span,
            },
            Stmt::Switch { value, arms, span } => Stmt::Switch {
                value: Box::new(self.expand_expr(value, bindings, site)?),
                arms: arms
                    .iter()
                    .map(|arm| match arm {
                        SwitchArm::Case { value, body, span } => Ok(SwitchArm::Case {
                            value: Box::new(self.expand_expr(value, bindings, site)?),
                            body: self.expand_stmts(body, bindings, site)?,
                            span: site.or(*span),
                        }),
                        SwitchArm::Default { body, span } => Ok(SwitchArm::Default {
                            body: self.expand_stmts(body, bindings, site)?,
                            span: site.or(*span),
                        }),
                    })
                    .collect::<Result<Vec<_>, IntegrationError>>()?,
                span: *span,
            },
            Stmt::Delete { target, span } => Stmt::Delete {
                target: Box::new(self.expand_expr(target, bindings, site)?),
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
                    .map(|offset| self.expand_expr(offset, bindings, site).map(Box::new))
                    .transpose()?,
                rule_start: *rule_start,
                span: *span,
            },
            Stmt::Break { .. }
            | Stmt::Return { .. }
            | Stmt::CallSubroutine { .. }
            | Stmt::Pass { .. }
            | Stmt::Continue { .. }
            | Stmt::Label { .. } => statement.clone(),
        })
    }

    pub(crate) fn expand_expr(
        &mut self,
        expression: &Expr,
        bindings: &MacroBindings,
        site: Option<HirSpan>,
    ) -> Result<Expr, IntegrationError> {
        let mut expanded = self.expand_expr_inner(expression, bindings, site)?;
        if let Some(site) = site {
            set_expr_span(&mut expanded, site);
        }
        Ok(expanded)
    }

    fn expand_expr_inner(
        &mut self,
        expression: &Expr,
        bindings: &MacroBindings,
        site: Option<HirSpan>,
    ) -> Result<Expr, IntegrationError> {
        match expression {
            Expr::MacroParam { name, span } => {
                let value = bindings.get(name).ok_or_else(|| {
                    IntegrationError::new(
                        "unsupported-integration-surface",
                        format!("macro parameter '{name}' has no expansion binding"),
                        *span,
                    )
                })?;
                if site.is_some() {
                    self.expand_expr(value, bindings, site)
                } else {
                    Ok(value.clone())
                }
            }
            Expr::MacroCall { name, args, span } => {
                let args = args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings, site))
                    .collect::<Result<Vec<_>, _>>()?;
                let body = self.expand_macro_body(name, &args, *span, site)?;
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
                    .map(|element| self.expand_expr(element, bindings, site))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::Dict { entries, span } => Ok(Expr::Dict {
                entries: entries
                    .iter()
                    .map(|entry| {
                        Ok(hir::DictEntry {
                            key: Box::new(self.expand_expr(&entry.key, bindings, site)?),
                            value: Box::new(self.expand_expr(&entry.value, bindings, site)?),
                            span: site.or(entry.span),
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
                element: Box::new(self.expand_expr(element, bindings, site)?),
                variable: variable.clone(),
                variable_span: site.or(*variable_span),
                index: index.clone(),
                index_span: site.or(*index_span),
                iterable: Box::new(self.expand_expr(iterable, bindings, site)?),
                condition: condition
                    .as_ref()
                    .map(|condition| self.expand_expr(condition, bindings, site).map(Box::new))
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
                param_spans: param_spans.iter().map(|span| site.or(*span)).collect(),
                body: Box::new(self.expand_expr(body, bindings, site)?),
                span: *span,
            }),
            Expr::Type { name, args, span } => Ok(Expr::Type {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings, site))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            }),
            Expr::Vector { x, y, z, span } => Ok(Expr::Vector {
                x: Box::new(self.expand_expr(x, bindings, site)?),
                y: Box::new(self.expand_expr(y, bindings, site)?),
                z: Box::new(self.expand_expr(z, bindings, site)?),
                span: *span,
            }),
            Expr::PlayerVar {
                player,
                name,
                member_span,
                span,
            } => Ok(Expr::PlayerVar {
                player: Box::new(self.expand_expr(player, bindings, site)?),
                name: name.clone(),
                member_span: site.or(*member_span),
                span: *span,
            }),
            Expr::Member {
                receiver,
                member,
                member_span,
                span,
            } => Ok(Expr::Member {
                receiver: Box::new(self.expand_expr(receiver, bindings, site)?),
                member: member.clone(),
                member_span: site.or(*member_span),
                span: *span,
            }),
            Expr::Call {
                name,
                args,
                debug_source,
                span,
            } => Ok(Expr::Call {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings, site))
                    .collect::<Result<Vec<_>, _>>()?,
                debug_source: debug_source.clone(),
                span: *span,
            }),
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                span,
            } => Ok(Expr::ReceiverCall {
                receiver: Box::new(self.expand_expr(receiver, bindings, site)?),
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings, site))
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
                left: Box::new(self.expand_expr(left, bindings, site)?),
                right: Box::new(self.expand_expr(right, bindings, site)?),
                span: *span,
            }),
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                span,
            } => Ok(Expr::Conditional {
                then_value: Box::new(self.expand_expr(then_value, bindings, site)?),
                condition: Box::new(self.expand_expr(condition, bindings, site)?),
                else_value: Box::new(self.expand_expr(else_value, bindings, site)?),
                span: *span,
            }),
            Expr::Unary { op, operand, span } => Ok(Expr::Unary {
                op: op.clone(),
                operand: Box::new(self.expand_expr(operand, bindings, site)?),
                span: *span,
            }),
            Expr::Index { array, index, span } => Ok(Expr::Index {
                array: Box::new(self.expand_expr(array, bindings, site)?),
                index: Box::new(self.expand_expr(index, bindings, site)?),
                span: *span,
            }),
            Expr::Format { text, args, span } => Ok(Expr::Format {
                text: text.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings, site))
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
        parent_site: Option<HirSpan>,
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
        let site = parent_site.or_else(|| self.attribute_to_site.then_some(span).flatten());
        let result = self.expand_stmts(&body, &bindings, site);
        self.stack.pop();
        result
    }
}

fn set_stmt_span(statement: &mut Stmt, site: HirSpan) {
    match statement {
        Stmt::Expr { span, .. }
        | Stmt::Assign { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::DoWhile { span, .. }
        | Stmt::Switch { span, .. }
        | Stmt::Delete { span, .. }
        | Stmt::Goto { span, .. }
        | Stmt::Break { span }
        | Stmt::Return { span }
        | Stmt::Continue { span }
        | Stmt::Label { span, .. }
        | Stmt::CallSubroutine { span, .. }
        | Stmt::Pass { span } => *span = Some(site),
    }
}

fn set_expr_span(expression: &mut Expr, site: HirSpan) {
    match expression {
        Expr::Number { span, .. }
        | Expr::String { span, .. }
        | Expr::Bool { span, .. }
        | Expr::Null { span }
        | Expr::StringModifier { span, .. }
        | Expr::Local { span, .. }
        | Expr::Enum { span, .. }
        | Expr::GlobalVar { span, .. }
        | Expr::HostPlayer { span }
        | Expr::EventPlayer { span }
        | Expr::Constant { span, .. }
        | Expr::MacroParam { span, .. }
        | Expr::Array { span, .. }
        | Expr::Dict { span, .. }
        | Expr::Comprehension { span, .. }
        | Expr::Lambda { span, .. }
        | Expr::Type { span, .. }
        | Expr::Vector { span, .. }
        | Expr::PlayerVar { span, .. }
        | Expr::Member { span, .. }
        | Expr::Call { span, .. }
        | Expr::ReceiverCall { span, .. }
        | Expr::Binary { span, .. }
        | Expr::Conditional { span, .. }
        | Expr::Unary { span, .. }
        | Expr::Index { span, .. }
        | Expr::Format { span, .. }
        | Expr::MacroCall { span, .. } => *span = Some(site),
    }
}
