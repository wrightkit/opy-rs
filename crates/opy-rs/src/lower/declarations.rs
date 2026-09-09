use super::*;

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
                initializer: self.initializer(initializer.as_ref()),
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
                span,
                name_span,
            } => Some(Declaration::Subroutine {
                name: name.clone(),
                index: None,
                span: Some(span.into()),
                name_span: Some(name_span.into()),
            }),
            Decl::Enum { .. } => None,
            Decl::Constant { name, value, span } => {
                let previous = self.allow_dict_literal;
                self.allow_dict_literal = true;
                let value = self.lower_expr(value, &[], CallPosition::Value);
                self.allow_dict_literal = previous;
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
                    self.enums.entry(name.clone()).or_insert_with(|| {
                        members.iter().map(|(member, _)| member.clone()).collect()
                    });
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
        self.subroutine_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
            || self
                .subroutine_definitions
                .iter()
                .any(|(definition, order)| definition == name && *order <= self.current_order)
    }

    pub(super) fn global_visible(&self, name: &str) -> bool {
        self.global_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
    }

    pub(super) fn player_visible(&self, name: &str) -> bool {
        self.player_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
    }

    pub(super) fn macro_visible(&self, name: &str) -> bool {
        self.macro_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
    }

    pub(super) fn constant_visible(&self, name: &str) -> bool {
        self.constant_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
    }

    pub(super) fn enum_visible(&self, name: &str) -> bool {
        self.enum_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
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
}
