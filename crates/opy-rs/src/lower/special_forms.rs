use super::*;

fn literal_number(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Number { value, .. } => Some(*value),
        Expr::Unary { op, operand, .. } if matches!(op.as_str(), "+" | "-") => {
            literal_number(operand).map(|value| if op == "-" { -value } else { value })
        }
        _ => None,
    }
}

fn compressed_literal_values(expr: &Expr) -> Option<Vec<f64>> {
    match expr {
        Expr::Null { .. } => Some(vec![0.0]),
        Expr::Number { .. } | Expr::Unary { .. } => literal_number(expr).map(|value| vec![value]),
        Expr::Call { name, args, .. } if name == "vect" && args.len() == 3 => Some(
            args.iter()
                .map(|arg| literal_number(&arg.value))
                .collect::<Option<Vec<_>>>()?,
        ),
        _ => None,
    }
}

fn is_compressible_column(values: &[&Expr]) -> bool {
    let Some(numbers) = values
        .iter()
        .map(|value| compressed_literal_values(value))
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let Some(first) = numbers.first() else {
        return false;
    };
    let is_vector = first.len() == 3;
    numbers.iter().all(|value| {
        value.len() == first.len()
            && (value.len() == 3) == is_vector
            && value
                .iter()
                .all(|component| component.abs() < if is_vector { 4999.0 } else { 49999.0 })
    })
}

impl Lowerer {
    pub(super) fn lower_tabular(
        &mut self,
        args: &[cst::CallArg],
        span: Span,
        macro_params: &[String],
    ) -> Vec<HirStmt> {
        let Some(cst::Expr::Array {
            elements: targets, ..
        }) = args.first().map(|arg| &arg.value)
        else {
            self.error_at(
                "tabular-arguments",
                "tabular first argument must be an array of variables".to_string(),
                span,
            );
            return Vec::new();
        };
        let Some(cst::Expr::Array {
            elements: values, ..
        }) = args.get(1).map(|arg| &arg.value)
        else {
            self.error_at(
                "tabular-arguments",
                "tabular second argument must be an array".to_string(),
                span,
            );
            return Vec::new();
        };
        if targets.is_empty() {
            self.error_at(
                "tabular-arguments",
                "tabular requires at least one target variable".to_string(),
                span,
            );
            return Vec::new();
        }
        if values.len() % targets.len() != 0 {
            self.error_at(
                "tabular-arguments",
                format!(
                    "tabular second argument must have a length that is a multiple of {} (length is {})",
                    targets.len(),
                    values.len()
                ),
                args.get(1).map_or(span, |arg| arg.value.span()),
            );
            return Vec::new();
        }
        let compress = matches!(
            args.get(2).map(|arg| &arg.value),
            Some(cst::Expr::Bool { value: true, .. })
        );
        let mut result = Vec::with_capacity(targets.len());
        for (column, target) in targets.iter().enumerate() {
            let target = self.lower_expr(target, macro_params, CallPosition::Value);
            let column_values_cst = values
                .iter()
                .skip(column)
                .step_by(targets.len())
                .collect::<Vec<_>>();
            let column_values = column_values_cst
                .iter()
                .map(|value| self.lower_expr(value, macro_params, CallPosition::Value))
                .collect();
            let value = HirExpr::Array {
                elements: column_values,
                span: Some(span.into()),
            };
            let value = if compress && is_compressible_column(&column_values_cst) {
                HirExpr::Call {
                    name: "compressed".to_string(),
                    args: vec![value],
                    debug_source: None,
                    span: Some(span.into()),
                }
            } else {
                value
            };
            result.push(HirStmt::Assign {
                target: Box::new(target),
                value: Box::new(value),
                span: Some(span.into()),
            });
        }
        result
    }

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

        let compress = matches!(
            args.get(2).map(|arg| &arg.value),
            Some(Expr::Bool { value: true, .. })
        );
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
            let values: Vec<HirExpr> = elements
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
            let compressible = compress && is_compressible(&values);
            let array = HirExpr::Array {
                elements: values,
                span: Some(span.into()),
            };
            let value = if compressible {
                HirExpr::Call {
                    name: "compressed".to_string(),
                    args: vec![array],
                    debug_source: None,
                    span: Some(span.into()),
                }
            } else {
                array
            };
            result.push(HirStmt::Assign {
                target: Box::new(target),
                value: Box::new(value),
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
            debug_source: None,
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

/// Whether `compressed()` accepts these elements: only numbers, or only
/// vectors of numbers.
fn is_compressible(values: &[HirExpr]) -> bool {
    fn number(expr: &HirExpr) -> Option<f64> {
        match expr {
            HirExpr::Number { value, .. } => Some(*value),
            HirExpr::Null { .. } => Some(0.0),
            HirExpr::Unary { op, operand, .. } if op == "-" => number(operand).map(|v| -v),
            HirExpr::Unary { op, operand, .. } if op == "+" => number(operand),
            _ => None,
        }
    }
    if values.is_empty() {
        return false;
    }
    let vectors = values
        .iter()
        .all(|value| matches!(value, HirExpr::Vector { .. }));
    if vectors {
        return values.iter().all(|value| {
            let HirExpr::Vector { x, y, z, .. } = value else {
                return false;
            };
            [x, y, z]
                .into_iter()
                .all(|component| number(component).is_some_and(|v| v.abs() < 4999.0))
        });
    }
    values
        .iter()
        .all(|value| number(value).is_some_and(|v| v.abs() < 49999.0))
}
