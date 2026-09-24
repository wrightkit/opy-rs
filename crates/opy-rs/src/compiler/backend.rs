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
            Expr::Call {
                name,
                args,
                debug_source,
                span,
            } => Ok(Expr::Call {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.expand_expr(arg, bindings))
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
        let mut result = result?;
        if let Some(site) = span.filter(|_| self.attribute_to_site) {
            relocate_stmts(&mut result, site);
        }
        Ok(result)
    }
}

/// Attribute every span of macro-expanded code to the invocation `site`, so
/// source attribution never points into the macro definition.
fn relocate_stmts(statements: &mut [Stmt], site: HirSpan) {
    for statement in statements {
        relocate_stmt(statement, site);
    }
}

fn relocate_stmt(statement: &mut Stmt, site: HirSpan) {
    let at = Some(site);
    match statement {
        Stmt::Expr { expr, span } | Stmt::Delete { target: expr, span } => {
            relocate_expr(expr, site);
            *span = at;
        }
        Stmt::Assign {
            target,
            value,
            span,
        } => {
            relocate_expr(target, site);
            relocate_expr(value, site);
            *span = at;
        }
        Stmt::If {
            branches,
            r#else,
            span,
        } => {
            for branch in branches {
                relocate_expr(&mut branch.condition, site);
                relocate_stmts(&mut branch.body, site);
            }
            if let Some(body) = r#else {
                relocate_stmts(body, site);
            }
            *span = at;
        }
        Stmt::For {
            variable,
            iterable,
            body,
            span,
        } => {
            relocate_expr(variable, site);
            relocate_expr(iterable, site);
            relocate_stmts(body, site);
            *span = at;
        }
        Stmt::While {
            condition,
            body,
            span,
        }
        | Stmt::DoWhile {
            condition,
            body,
            span,
        } => {
            relocate_expr(condition, site);
            relocate_stmts(body, site);
            *span = at;
        }
        Stmt::Switch { value, arms, span } => {
            relocate_expr(value, site);
            for arm in arms {
                match arm {
                    SwitchArm::Case { value, body, span } => {
                        relocate_expr(value, site);
                        relocate_stmts(body, site);
                        *span = at;
                    }
                    SwitchArm::Default { body, span } => {
                        relocate_stmts(body, site);
                        *span = at;
                    }
                }
            }
            *span = at;
        }
        Stmt::Goto { offset, span, .. } => {
            if let Some(offset) = offset {
                relocate_expr(offset, site);
            }
            *span = at;
        }
        Stmt::Break { span }
        | Stmt::Return { span }
        | Stmt::Continue { span }
        | Stmt::Label { span, .. }
        | Stmt::CallSubroutine { span, .. }
        | Stmt::Pass { span } => *span = at,
    }
}

fn relocate_expr(expression: &mut Expr, site: HirSpan) {
    let at = Some(site);
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
        | Expr::MacroParam { span, .. } => *span = at,
        Expr::Array {
            elements: args,
            span,
        }
        | Expr::Call { args, span, .. }
        | Expr::MacroCall { args, span, .. }
        | Expr::Type { args, span, .. }
        | Expr::Format { args, span, .. } => {
            for arg in args {
                relocate_expr(arg, site);
            }
            *span = at;
        }
        Expr::Dict { entries, span } => {
            for entry in entries {
                relocate_expr(&mut entry.key, site);
                relocate_expr(&mut entry.value, site);
                entry.span = at;
            }
            *span = at;
        }
        Expr::Comprehension {
            element,
            variable_span,
            index_span,
            iterable,
            condition,
            span,
            ..
        } => {
            relocate_expr(element, site);
            relocate_expr(iterable, site);
            if let Some(condition) = condition {
                relocate_expr(condition, site);
            }
            *variable_span = at;
            *index_span = at;
            *span = at;
        }
        Expr::Lambda {
            param_spans,
            body,
            span,
            ..
        } => {
            relocate_expr(body, site);
            param_spans.iter_mut().for_each(|param| *param = at);
            *span = at;
        }
        Expr::Vector { x, y, z, span } => {
            for component in [x, y, z] {
                relocate_expr(component, site);
            }
            *span = at;
        }
        Expr::PlayerVar {
            player: receiver,
            member_span,
            span,
            ..
        }
        | Expr::Member {
            receiver,
            member_span,
            span,
            ..
        } => {
            relocate_expr(receiver, site);
            *member_span = at;
            *span = at;
        }
        Expr::ReceiverCall {
            receiver,
            args,
            span,
            ..
        } => {
            relocate_expr(receiver, site);
            for arg in args {
                relocate_expr(arg, site);
            }
            *span = at;
        }
        Expr::Binary {
            left, right, span, ..
        } => {
            relocate_expr(left, site);
            relocate_expr(right, site);
            *span = at;
        }
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            span,
        } => {
            for part in [then_value, condition, else_value] {
                relocate_expr(part, site);
            }
            *span = at;
        }
        Expr::Unary { operand, span, .. } => {
            relocate_expr(operand, site);
            *span = at;
        }
        Expr::Index { array, index, span } => {
            relocate_expr(array, site);
            relocate_expr(index, site);
            *span = at;
        }
    }
}
