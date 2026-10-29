use super::*;

fn declaration_visible(
    declarations: &HashMap<String, usize>,
    name: &str,
    current_order: usize,
) -> bool {
    declarations
        .get(name)
        .is_some_and(|order| *order <= current_order)
}

impl Lowerer {
    pub(super) fn lower_declaration(&mut self, decl: &Decl) -> Option<Declaration> {
        match decl {
            Decl::GlobalVariable {
                name,
                index,
                span,
                name_span,
                initializer,
            } => Some(Declaration::GlobalVariable {
                name: name.clone(),
                index: *index,
                span: Some(span.into()),
                name_span: Some(name_span.into()),
                initializer: self.global_initializer(initializer.as_ref()),
            }),
            Decl::PlayerVariable {
                name,
                index,
                span,
                name_span,
                initializer,
            } => Some(Declaration::PlayerVariable {
                name: name.clone(),
                index: *index,
                span: Some(span.into()),
                name_span: Some(name_span.into()),
                initializer: self.initializer(initializer.as_ref()),
            }),
            Decl::Subroutine {
                name,
                index,
                span,
                name_span,
            } => Some(Declaration::Subroutine {
                name: name.clone(),
                index: *index,
                span: Some(span.into()),
                name_span: Some(name_span.into()),
            }),
            Decl::Enum { .. } => None,
            Decl::Constant { name, value, span } => {
                let value =
                    self.with_dict_literal(|s| s.lower_expr(value, &[], CallPosition::Value));
                Some(Declaration::Constant {
                    name: name.clone(),
                    span: Some(span.into()),
                    value: Box::new(value),
                })
            }
            Decl::Macro {
                name,
                args,
                body,
                span,
            } => Some(Declaration::Macro {
                name: name.clone(),
                args: args.clone(),
                span: Some(span.into()),
                body: self.lower_macro_body(body, args),
            }),
        }
    }

    pub(super) fn collect_symbols(&mut self, program: &cst::Program) {
        for (order, item) in program.top_level.iter().enumerate() {
            let TopLevel::Declaration(decl) = item else {
                continue;
            };
            match decl {
                Decl::GlobalVariable { name, span, .. } => {
                    let duplicate = self.global_declarations.contains_key(name);
                    self.global_declarations
                        .entry(name.clone())
                        .or_insert(order);
                    if duplicate {
                        self.error_at(
                            "duplicate-declaration",
                            format!("duplicate global variable '{name}'"),
                            *span,
                        );
                    }
                }
                Decl::PlayerVariable { name, span, .. } => {
                    let duplicate = self.player_declarations.contains_key(name);
                    self.player_declarations
                        .entry(name.clone())
                        .or_insert(order);
                    if duplicate {
                        self.error_at(
                            "duplicate-declaration",
                            format!("duplicate player variable '{name}'"),
                            *span,
                        );
                    }
                }
                Decl::Subroutine { name, span, .. } => {
                    let duplicate = self.subroutine_declarations.contains_key(name);
                    self.subroutine_declarations
                        .entry(name.clone())
                        .or_insert(order);
                    if duplicate {
                        self.error_at(
                            "duplicate-declaration",
                            format!("duplicate subroutine '{name}'"),
                            *span,
                        );
                    }
                }
                Decl::Constant { name, span, .. } => {
                    let duplicate = self.constant_declarations.contains_key(name);
                    self.constant_declarations
                        .entry(name.clone())
                        .or_insert(order);
                    if duplicate {
                        self.error_at(
                            "duplicate-declaration",
                            format!("duplicate constant '{name}'"),
                            *span,
                        );
                    }
                }
                Decl::Enum { name, members, .. } => {
                    self.enum_declarations.entry(name.clone()).or_insert(order);
                    if !self.enums.contains_key(name) {
                        let resolved = self.enum_members(members);
                        self.enums.insert(name.clone(), resolved);
                    }
                }
                Decl::Macro { name, .. } => {
                    self.macro_declarations.entry(name.clone()).or_insert(order);
                }
            }
        }
        for (order, item) in program.top_level.iter().enumerate() {
            let TopLevel::Rule(CstRuleEntry::SubroutineDef { name, span, .. }) = item else {
                continue;
            };
            if self
                .subroutine_definitions
                .iter()
                .any(|(defined, _)| defined == name)
            {
                self.error_at(
                    "duplicate-definition",
                    format!("duplicate subroutine definition '{name}'"),
                    *span,
                );
            }
            self.subroutine_definitions.push((name.clone(), order));
        }
    }

    pub(super) fn subroutine_visible(&self, name: &str) -> bool {
        declaration_visible(&self.subroutine_declarations, name, self.current_order)
            || self
                .subroutine_definitions
                .iter()
                .any(|(definition, order)| definition == name && *order <= self.current_order)
    }

    pub(super) fn global_visible(&self, name: &str) -> bool {
        declaration_visible(&self.global_declarations, name, self.current_order)
    }

    pub(super) fn player_visible(&self, name: &str) -> bool {
        declaration_visible(&self.player_declarations, name, self.current_order)
    }

    pub(super) fn macro_visible(&self, name: &str) -> bool {
        declaration_visible(&self.macro_declarations, name, self.current_order)
    }

    pub(super) fn constant_visible(&self, name: &str) -> bool {
        declaration_visible(&self.constant_declarations, name, self.current_order)
    }

    pub(super) fn enum_visible(&self, name: &str) -> bool {
        declaration_visible(&self.enum_declarations, name, self.current_order)
    }

    fn global_initializer(&mut self, initializer: Option<&Expr>) -> Option<Box<HirExpr>> {
        use crate::hir::visit::{self, Visitor};

        struct EventPlayerCheck<'a>(&'a mut Lowerer, Span);

        impl Visitor for EventPlayerCheck<'_> {
            fn visit_expr(&mut self, expression: &HirExpr) {
                if matches!(expression, HirExpr::EventPlayer { .. }) {
                    self.0.error_at(
                        "rule-event-gate",
                        "Cannot use 'eventPlayer' with rule event 'global'".to_string(),
                        self.1,
                    );
                }
                visit::walk_expr(self, expression);
            }
        }

        let initializer = initializer?;
        let value = self.initializer(Some(initializer))?;
        EventPlayerCheck(self, initializer.span()).visit_expr(&value);
        Some(value)
    }

    /// A declaration initializer: integer-`0` literal initializers are
    /// dropped (matching the reference adapter, which drops `h = 0` but
    /// carries `j = 5` and `k = 0.0`); other initializers are kept.
    pub(super) fn initializer(&mut self, initializer: Option<&Expr>) -> Option<Box<HirExpr>> {
        let initializer = initializer?;
        let lowered = self.lower_expr(initializer, &[], CallPosition::Value);
        match &lowered {
            HirExpr::Number { text, .. } if text == "0" => None,
            other => Some(Box::new(other.clone())),
        }
    }

    /// Resolves one enum's members following the reference's `lastIntValue`
    /// semantics: a bare member takes the running counter, `= <number>`
    /// reseeds the counter at `value + 1`, `= -<number>` yields `-value + 1`
    /// for the next bare member only, and any other explicit expression is
    /// stored for inlining while breaking the numeric chain.
    fn enum_members(&mut self, members: &[(String, Option<Expr>, Span)]) -> Vec<EnumMember> {
        let mut counter = EnumCounter::Number(0.0);
        let mut previous: Option<&Expr> = None;
        members
            .iter()
            .map(|(name, value, span)| {
                let resolved = match value {
                    Some(expr) => {
                        counter = match expr {
                            Expr::Number { value, .. } => EnumCounter::Number(value + 1.0),
                            Expr::Unary { op, operand, .. }
                                if op == "-" && matches!(operand.as_ref(), Expr::Number { .. }) =>
                            {
                                let Expr::Number { value, .. } = operand.as_ref() else {
                                    unreachable!()
                                };
                                EnumCounter::Negated(*value)
                            }
                            _ => EnumCounter::Other,
                        };
                        previous = Some(expr);
                        EnumMemberValue::Expr(expr.clone())
                    }
                    None => {
                        let next = match counter {
                            EnumCounter::Number(next) => {
                                counter = EnumCounter::Number(next + 1.0);
                                Some(next)
                            }
                            EnumCounter::Negated(inner) => {
                                counter = EnumCounter::Number(-inner + 2.0);
                                Some(-inner + 1.0)
                            }
                            EnumCounter::Other => {
                                self.error_at(
                                    "enum-increment",
                                    format!(
                                        "cannot auto-increment enum member, as last value was {}",
                                        enum_value_display(previous)
                                    ),
                                    *span,
                                );
                                counter = EnumCounter::Number(0.0);
                                None
                            }
                        };
                        previous = None;
                        next.map_or(EnumMemberValue::Number(0.0), EnumMemberValue::Number)
                    }
                };
                EnumMember {
                    name: name.clone(),
                    value: resolved,
                }
            })
            .collect()
    }
}

/// Renders an enum member's explicit value the way the reference's
/// `functionNameToString` describes the breaking predecessor in its
/// `Cannot auto-increment` error.
fn enum_value_display(value: Option<&Expr>) -> String {
    let Some(expr) = value else {
        return "function 'unknown'".to_string();
    };
    let binary = |op: &str| -> String {
        let operand = match op {
            "+" => "'+' or '+='",
            "-" => "'-' or '-='",
            "*" => "'*' or '*='",
            "/" => "'/' or '/='",
            "%" => "'%' or '%='",
            "**" => "'**' or '**='",
            "in" => "'in'",
            "and" => "'and'",
            "or" => "'or'",
            "not" => "'not'",
            "==" => "'=='",
            "!=" => "'!='",
            ">" => "'>'",
            ">=" => "'>='",
            "<" => "'<'",
            "<=" => "'<='",
            _ => return format!("function '{op}'"),
        };
        format!("operator {operand}")
    };
    match expr {
        Expr::Number { value, .. } => {
            format!(
                "number '{}'",
                crate::compiler::number_format::javascript_text(*value)
            )
        }
        Expr::String { .. } | Expr::StringModifier { .. } => {
            "function '__customString__'".to_string()
        }
        Expr::Bool { .. } => "function 'bool'".to_string(),
        Expr::Null { .. } => "function 'null'".to_string(),
        Expr::Array { .. } | Expr::Comprehension { .. } => "function '__array__'".to_string(),
        Expr::Dict { .. } => "function '__dict__'".to_string(),
        Expr::Lambda { .. } => "function 'lambda'".to_string(),
        Expr::Call { name, .. } | Expr::ReceiverCall { name, .. } => format!("function '{name}'"),
        Expr::Name { name, .. } => format!("global variable '{name}'"),
        Expr::Member { receiver, .. } => match receiver.as_ref() {
            Expr::Name { name, .. } => format!("enum '{name}'"),
            _ => "function 'member'".to_string(),
        },
        Expr::Type { name, .. } => format!("function '{name}'"),
        Expr::Index { .. } => "function 'valueInArray'".to_string(),
        Expr::Binary { op, .. } => binary(op),
        Expr::Conditional { .. } => "function 'condition'".to_string(),
        Expr::Unary { op, .. } => match op.as_str() {
            "-" => "function '__negate__'".to_string(),
            other => binary(other),
        },
    }
}
