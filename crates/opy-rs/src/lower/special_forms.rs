use super::*;

impl Lowerer {
    pub(super) fn lower_split_dict_array(
        &mut self,
        args: &[cst::CallArg],
        span: Span,
        macro_params: &[String],
    ) -> Vec<HirStmt> {
        let Some(fields) = args.first().map(|arg| &arg.value) else {
            self.error_at(
                "split-dict-array-arity",
                "splitDictArray requires a field mapping and data array".to_string(),
                span,
            );
            return Vec::new();
        };
        let Some(rows) = args.get(1).map(|arg| &arg.value) else {
            self.error_at(
                "split-dict-array-arity",
                "splitDictArray requires a field mapping and data array".to_string(),
                span,
            );
            return Vec::new();
        };
        let Expr::Dict {
            entries: field_entries,
            ..
        } = fields
        else {
            self.error_at(
                "split-dict-array-arguments",
                "splitDictArray first argument must be a dictionary".to_string(),
                fields.span(),
            );
            return Vec::new();
        };
        let Expr::Array { elements, .. } = rows else {
            self.error_at(
                "split-dict-array-arguments",
                "splitDictArray second argument must be an array".to_string(),
                rows.span(),
            );
            return Vec::new();
        };

        let mut result = Vec::with_capacity(field_entries.len());
        for field in field_entries {
            let Some(field_name) = expr_identifier(&field.key) else {
                self.error_at(
                    "split-dict-array-key",
                    "splitDictArray field keys must be identifiers".to_string(),
                    field.key.span(),
                );
                continue;
            };
            let target = self.lower_expr(&field.value, macro_params, CallPosition::Value);
            let previous = self.allow_dict_literal;
            self.allow_dict_literal = true;
            let values = elements
                .iter()
                .map(|element| match element {
                    Expr::Dict { entries, .. } => entries
                        .iter()
                        .find(|entry| expr_identifier(&entry.key) == Some(field_name))
                        .map(|entry| {
                            self.lower_expr(&entry.value, macro_params, CallPosition::Value)
                        })
                        .unwrap_or(HirExpr::Null { span: None }),
                    _ => HirExpr::Null { span: None },
                })
                .collect();
            self.allow_dict_literal = previous;
            result.push(HirStmt::Assign {
                target: Box::new(target),
                value: Box::new(HirExpr::Array {
                    elements: values,
                    span: Some(span.into()),
                }),
                span: Some(span.into()),
            });
        }
        result
    }

    pub(super) fn lower_macro_body(&mut self, body: &[Stmt], params: &[String]) -> Vec<HirStmt> {
        body.iter()
            .map(|stmt| match stmt {
                Stmt::Expr { expr, span } => HirStmt::Expr {
                    expr: Box::new(self.lower_expr(expr, params, CallPosition::MacroBody)),
                    span: Some(span.into()),
                },
                _ => self.lower_stmt(stmt, params, false, false),
            })
            .collect()
    }

    pub(super) fn lower_workshop_setting(
        &mut self,
        args: &[cst::CallArg],
        span: Span,
        macro_params: &[String],
    ) -> HirExpr {
        if !(4..=5).contains(&args.len()) {
            self.error_at(
                "invalid-arity",
                format!(
                    "function 'createWorkshopSetting' takes 4 or 5 arguments, received {}",
                    args.len()
                ),
                span,
            );
            return HirExpr::Null { span: None };
        }
        for arg in args {
            if let Some((keyword, keyword_span)) = &arg.keyword {
                self.error_at(
                    "keyword-unsupported",
                    format!(
                        "function 'createWorkshopSetting' does not accept keyword arguments ('{keyword}')"
                    ),
                    *keyword_span,
                );
            }
        }

        let setting_type = match &args[0].value {
            Expr::Type {
                name,
                args: type_args,
                span: type_span,
            } => HirExpr::Type {
                name: name.clone(),
                args: type_args
                    .iter()
                    .map(|arg| self.lower_expr(arg, macro_params, CallPosition::Value))
                    .collect(),
                span: Some((*type_span).into()),
            },
            Expr::Name {
                name,
                span: type_span,
            } if matches!(name.as_str(), "bool" | "int" | "float") => HirExpr::Type {
                name: name.clone(),
                args: Vec::new(),
                span: Some((*type_span).into()),
            },
            other => {
                self.error_at(
                    "invalid-argument",
                    "argument 1 of 'createWorkshopSetting' must be a setting type".to_string(),
                    other.span(),
                );
                HirExpr::Null { span: None }
            }
        };
        let mut lowered = Vec::with_capacity(5);
        lowered.push(setting_type);
        lowered.extend(
            args[1..]
                .iter()
                .map(|arg| self.lower_expr(&arg.value, macro_params, CallPosition::Value)),
        );
        if args.len() == 4 {
            lowered.push(HirExpr::Number {
                value: 0.0,
                text: "0".to_string(),
                span: None,
            });
        }
        HirExpr::Call {
            name: "createWorkshopSetting".to_string(),
            args: lowered,
            span: Some(span.into()),
        }
    }

    pub(super) fn qualified_macro_for_member(&self, member: &str) -> Option<String> {
        let suffix = format!(".{member}");
        self.macro_declarations
            .iter()
            .filter(|(name, order)| name.ends_with(&suffix) && **order <= self.current_order)
            .map(|(name, _)| name.clone())
            .next()
    }
}
