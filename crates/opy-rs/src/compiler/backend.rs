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

pub(crate) type MacroBindings<'a> = HashMap<&'a str, Expr>;

pub(crate) struct MacroExpander<'a> {
    macros: HashMap<&'a str, (&'a [String], &'a [Stmt])>,
    stack: Vec<String>,
    /// Relocate expanded spans to the invocation site; only the source
    /// attribution pass sets it, so diagnostics keep the definition spans.
    attribute_to_site: bool,
}

impl<'a> MacroExpander<'a> {
    pub(crate) fn from_declarations(declarations: &'a [hir::Declaration]) -> Self {
        let macros = declarations
            .iter()
            .filter_map(|declaration| match declaration {
                hir::Declaration::Macro {
                    name, args, body, ..
                } => Some((name.as_str(), (args.as_slice(), body.as_slice()))),
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
    let mut expander = MacroExpander::from_declarations(&program.declarations);
    expander.attribute_to_site = attribute_to_site;
    let mut expanded = program.clone();
    let bindings = MacroBindings::new();

    for declaration in &mut expanded.declarations {
        match declaration {
            hir::Declaration::GlobalVariable { initializer, .. }
            | hir::Declaration::PlayerVariable { initializer, .. } => {
                if let Some(initializer) = initializer {
                    expander.expand_expr_in_place(initializer, &bindings, None)?;
                }
            }
            _ => {}
        }
    }
    for entry in &mut expanded.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for argument in &mut rule.event.args {
                    expander.expand_expr_in_place(argument, &bindings, None)?;
                }
                for condition in &mut rule.conditions {
                    expander.expand_expr_in_place(condition, &bindings, None)?;
                }
                expander.expand_stmts(&mut rule.actions, &bindings, None)?;
            }
            RuleEntry::SubroutineDef { body, .. } => {
                expander.expand_stmts(body, &bindings, None)?;
            }
        }
    }
    Ok(expanded)
}

impl<'a> MacroExpander<'a> {
    fn expand_stmts(
        &mut self,
        statements: &mut Vec<Stmt>,
        bindings: &MacroBindings<'_>,
        site: Option<HirSpan>,
    ) -> Result<(), IntegrationError> {
        let mut expanded = Vec::new();
        for mut statement in statements.drain(..) {
            if let Stmt::Expr { expr, .. } = &mut statement {
                if let Expr::MacroCall { name, args, span } = expr.as_mut() {
                    for argument in args.iter_mut() {
                        self.expand_expr_in_place(argument, bindings, site)?;
                    }
                    expanded.extend(self.expand_macro_body(name, args, *span, site)?);
                    continue;
                }
            }
            self.expand_stmt_inner(&mut statement, bindings, site)?;
            if let Some(site) = site {
                set_stmt_span(&mut statement, site);
            }
            expanded.push(statement);
        }
        *statements = expanded;
        Ok(())
    }

    fn expand_stmt_inner(
        &mut self,
        statement: &mut Stmt,
        bindings: &MacroBindings<'_>,
        site: Option<HirSpan>,
    ) -> Result<(), IntegrationError> {
        match statement {
            Stmt::Expr { expr, .. } => self.expand_expr_in_place(expr, bindings, site)?,
            Stmt::Assign { target, value, .. } => {
                self.expand_expr_in_place(target, bindings, site)?;
                self.expand_expr_in_place(value, bindings, site)?;
            }
            Stmt::If {
                branches, r#else, ..
            } => {
                for branch in branches {
                    self.expand_expr_in_place(&mut branch.condition, bindings, site)?;
                    self.expand_stmts(&mut branch.body, bindings, site)?;
                }
                if let Some(body) = r#else {
                    self.expand_stmts(body, bindings, site)?;
                }
            }
            Stmt::For {
                variable,
                iterable,
                body,
                ..
            } => {
                self.expand_expr_in_place(variable, bindings, site)?;
                self.expand_expr_in_place(iterable, bindings, site)?;
                self.expand_stmts(body, bindings, site)?;
            }
            Stmt::While {
                condition, body, ..
            }
            | Stmt::DoWhile {
                condition, body, ..
            } => {
                self.expand_expr_in_place(condition, bindings, site)?;
                self.expand_stmts(body, bindings, site)?;
            }
            Stmt::Switch { value, arms, .. } => {
                self.expand_expr_in_place(value, bindings, site)?;
                for arm in arms {
                    match arm {
                        SwitchArm::Case { value, body, span } => {
                            self.expand_expr_in_place(value, bindings, site)?;
                            self.expand_stmts(body, bindings, site)?;
                            *span = site.or(*span);
                        }
                        SwitchArm::Default { body, span } => {
                            self.expand_stmts(body, bindings, site)?;
                            *span = site.or(*span);
                        }
                    }
                }
            }
            Stmt::Delete { target, .. } => self.expand_expr_in_place(target, bindings, site)?,
            Stmt::Goto { offset, .. } => {
                if let Some(offset) = offset {
                    self.expand_expr_in_place(offset, bindings, site)?;
                }
            }
            Stmt::Break { .. }
            | Stmt::Return { .. }
            | Stmt::Continue { .. }
            | Stmt::Label { .. }
            | Stmt::CallSubroutine { .. }
            | Stmt::Pass { .. } => {}
        }
        Ok(())
    }

    pub(crate) fn expand_expr(
        &mut self,
        expression: &Expr,
        bindings: &MacroBindings<'_>,
        site: Option<HirSpan>,
    ) -> Result<Expr, IntegrationError> {
        let mut expanded = expression.clone();
        self.expand_expr_in_place(&mut expanded, bindings, site)?;
        Ok(expanded)
    }

    fn expand_expr_in_place(
        &mut self,
        expression: &mut Expr,
        bindings: &MacroBindings<'_>,
        site: Option<HirSpan>,
    ) -> Result<(), IntegrationError> {
        let replacement = match expression {
            Expr::MacroParam { name, span } => {
                let value = bindings.get(name.as_str()).ok_or_else(|| {
                    IntegrationError::new(
                        "unsupported-integration-surface",
                        format!("macro parameter '{name}' has no expansion binding"),
                        *span,
                    )
                })?;
                Some(if site.is_some() {
                    self.expand_expr(value, bindings, site)?
                } else {
                    value.clone()
                })
            }
            Expr::MacroCall { name, args, span } => {
                for argument in args.iter_mut() {
                    self.expand_expr_in_place(argument, bindings, site)?;
                }
                let body = self.expand_macro_body(name, args, *span, site)?;
                if body.len() != 1 {
                    return Err(IntegrationError::new(
                        "macro-invalid",
                        format!("macro '{name}' must produce one expression in value position"),
                        *span,
                    ));
                }
                Some(
                    match body.into_iter().next().expect("one macro body statement") {
                        Stmt::Expr { expr, .. } => *expr,
                        _ => {
                            return Err(IntegrationError::new(
                                "macro-invalid",
                                format!(
                                    "macro '{name}' must produce an expression in value position"
                                ),
                                *span,
                            ));
                        }
                    },
                )
            }
            _ => None,
        };
        if let Some(replacement) = replacement {
            *expression = replacement;
            if let Some(site) = site {
                set_expr_span(expression, site);
            }
            return Ok(());
        }

        match expression {
            Expr::Array { elements, .. } => {
                for element in elements {
                    self.expand_expr_in_place(element, bindings, site)?;
                }
            }
            Expr::Dict { entries, .. } => {
                for entry in entries {
                    self.expand_expr_in_place(&mut entry.key, bindings, site)?;
                    self.expand_expr_in_place(&mut entry.value, bindings, site)?;
                    entry.span = site.or(entry.span);
                }
            }
            Expr::Comprehension {
                element,
                variable_span,
                index_span,
                iterable,
                condition,
                ..
            } => {
                self.expand_expr_in_place(element, bindings, site)?;
                self.expand_expr_in_place(iterable, bindings, site)?;
                if let Some(condition) = condition {
                    self.expand_expr_in_place(condition, bindings, site)?;
                }
                *variable_span = site.or(*variable_span);
                *index_span = site.or(*index_span);
            }
            Expr::Lambda {
                param_spans, body, ..
            } => {
                self.expand_expr_in_place(body, bindings, site)?;
                for span in param_spans {
                    *span = site.or(*span);
                }
            }
            Expr::Type { args, .. } | Expr::Call { args, .. } | Expr::Format { args, .. } => {
                for argument in args {
                    self.expand_expr_in_place(argument, bindings, site)?;
                }
            }
            Expr::Vector { x, y, z, .. } => {
                self.expand_expr_in_place(x, bindings, site)?;
                self.expand_expr_in_place(y, bindings, site)?;
                self.expand_expr_in_place(z, bindings, site)?;
            }
            Expr::PlayerVar {
                player,
                member_span,
                ..
            } => {
                self.expand_expr_in_place(player, bindings, site)?;
                *member_span = site.or(*member_span);
            }
            Expr::Member {
                receiver,
                member_span,
                ..
            } => {
                self.expand_expr_in_place(receiver, bindings, site)?;
                *member_span = site.or(*member_span);
            }
            Expr::ReceiverCall { receiver, args, .. } => {
                self.expand_expr_in_place(receiver, bindings, site)?;
                for argument in args {
                    self.expand_expr_in_place(argument, bindings, site)?;
                }
            }
            Expr::Binary { left, right, .. } => {
                self.expand_expr_in_place(left, bindings, site)?;
                self.expand_expr_in_place(right, bindings, site)?;
            }
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                ..
            } => {
                self.expand_expr_in_place(then_value, bindings, site)?;
                self.expand_expr_in_place(condition, bindings, site)?;
                self.expand_expr_in_place(else_value, bindings, site)?;
            }
            Expr::Unary { operand, .. } => {
                self.expand_expr_in_place(operand, bindings, site)?;
            }
            Expr::Index { array, index, .. } => {
                self.expand_expr_in_place(array, bindings, site)?;
                self.expand_expr_in_place(index, bindings, site)?;
            }
            Expr::Number { .. }
            | Expr::String { .. }
            | Expr::Bool { .. }
            | Expr::Null { .. }
            | Expr::StringModifier { .. }
            | Expr::Local { .. }
            | Expr::Enum { .. }
            | Expr::GlobalVar { .. }
            | Expr::HostPlayer { .. }
            | Expr::EventPlayer { .. }
            | Expr::Constant { .. }
            | Expr::MacroCall { .. }
            | Expr::MacroParam { .. } => {}
        }
        if let Some(site) = site {
            set_expr_span(expression, site);
        }
        Ok(())
    }

    fn expand_macro_body(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
        parent_site: Option<HirSpan>,
    ) -> Result<Vec<Stmt>, IntegrationError> {
        let Some((params, template)) = self.macros.get(name) else {
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
        for (param, arg) in params.iter().zip(args.iter()) {
            bindings.insert(param, arg.clone());
        }
        self.stack.push(name.to_string());
        let site = parent_site.or_else(|| self.attribute_to_site.then_some(span).flatten());
        let mut body = template.to_vec();
        let result = self.expand_stmts(&mut body, &bindings, site).map(|()| body);
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
