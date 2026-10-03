use super::*;

impl<'a> Lowering<'a> {
    pub(super) fn lower_translation_helper(
        &mut self,
        translations: &hir::TranslationState,
    ) -> Result<ValueId, IntegrationError> {
        let translated_white = translations
            .languages
            .iter()
            .map(|language| {
                let locale = match language.as_str() {
                    "de" => "de-DE",
                    "en" => "en-US",
                    "es" | "es_mx" => "es-MX",
                    "es_es" => "es-ES",
                    "fr" => "fr-FR",
                    "it" => "it-IT",
                    "ja" => "ja-JP",
                    "ko" => "ko-KR",
                    "pl" => "pl-PL",
                    "pt" => "pt-BR",
                    "ru" => "ru-RU",
                    "th" => "th-TH",
                    "tr" => "tr-TR",
                    "zh" | "zh_cn" => "zh-CN",
                    "zh_tw" => "zh-TW",
                    _ => {
                        return Err(IntegrationError::new(
                            "translations-invalid",
                            format!("unsupported translation language '{language}'"),
                            translations.span,
                        ));
                    }
                };
                self.compiler
                    .catalog
                    .localized_enum_spelling(
                        "Color",
                        &workshop_rs::catalog::Locale::new(locale),
                        "WHITE",
                    )
                    .ok_or_else(|| {
                        IntegrationError::new(
                            "translations-invalid",
                            format!("unsupported translation locale '{locale}'"),
                            translations.span,
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("0");
        let text = self.push_value(Value::String(format!("\u{ec48}0{translated_white}")));
        let custom_string = self.push_call("customString", vec![text]);
        let null = self.push_value(Value::Null);
        let separator = self.push_call("firstOf", vec![null]);
        Ok(self.push_call("stringSplit", vec![custom_string, separator]))
    }

    pub(super) fn lower_translation(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let Some(translations) = self.hir.preprocessing.translations.as_ref() else {
            return Err(IntegrationError::new(
                "translations-invalid",
                format!("translation function '{name}' requires #!translations"),
                span,
            ));
        };
        let (context, target) = match args {
            [target] => (None, target),
            [Expr::String { value: context, .. }, target] => (Some(context.as_str()), target),
            _ => {
                return Err(IntegrationError::new(
                    "translations-invalid",
                    format!("translation function '{name}' expects one or two arguments"),
                    span,
                ));
            }
        };
        let (literal, format_args) = match target {
            Expr::String { value, .. } => (value.clone(), Vec::new()),
            Expr::Format { text, args, .. } => {
                let (text, args) = self.fold_format_constants(text, args);
                (text, args)
            }
            _ => {
                let target = self.lower_value(target)?;
                if name == "___" {
                    return Ok(target);
                }
                return Ok(self.select_translation(target));
            }
        };
        if format_args.len() > 16 {
            return Err(IntegrationError::new(
                "translations-invalid",
                "translated format strings support at most sixteen dynamic arguments",
                span,
            ));
        }
        let literal = literal.as_str();
        let msgid = literal.trim();
        if literal.contains('\u{ec48}') {
            return Err(IntegrationError::new(
                "translations-invalid",
                "translation strings must not contain the reserved translation separator",
                span,
            ));
        }
        if !self
            .translation_uses
            .iter()
            .any(|(existing_msgid, existing)| {
                existing_msgid == msgid && existing.as_deref() == context
            })
        {
            self.translation_uses
                .push((msgid.to_string(), context.map(str::to_string)));
        }
        let use_tl_err = !self.translation_player_options().2;
        let mut localized = translations
            .languages
            .iter()
            .map(|language| {
                translations
                    .entries
                    .iter()
                    .find(|entry| entry.msgid == msgid && entry.context.as_deref() == context)
                    .and_then(|entry| entry.translations.get(language))
                    .filter(|value| !value.is_empty())
                    .cloned()
                    .unwrap_or_else(|| literal.to_string())
            })
            .collect::<Vec<_>>();
        let tl_err_prefix = if use_tl_err {
            "\u{ff34}\u{ff2c}\u{ff25}\u{ff52}\u{ff52}\u{ec48}"
        } else {
            ""
        };
        let raw_string = format!("{tl_err_prefix}{}", localized.join("\u{ec48}"));
        let replacement_mode = raw_string.chars().count() > 128 || format_args.len() > 3;
        if replacement_mode {
            for index in 0..format_args.len() {
                let marker = format_number_marker(index);
                for value in &mut localized {
                    *value = value.replace(&format!("{{{index}}}"), &marker);
                }
            }
            let encoded_segments = localized.iter().enumerate().map(|(index, value)| {
                if index == 0 {
                    format!("{tl_err_prefix}{value}")
                } else {
                    value.clone()
                }
            });
            for (index, segment) in encoded_segments.enumerate() {
                if segment.len() > 511 {
                    return Err(IntegrationError::new(
                        "translations-invalid",
                        format!(
                            "translated string for language '{}' is too long, maximum length is 511 bytes",
                            translations.languages[index]
                        ),
                        span,
                    ));
                }
            }
        }
        let encoded = format!("{tl_err_prefix}{}", localized.join("\u{ec48}"));
        let text = self.push_value(Value::String(encoded));
        let custom = if replacement_mode {
            let mut value = self.push_call("customString", vec![text]);
            for (index, arg) in format_args.iter().enumerate() {
                let marker = self.push_number(format_number_marker_value(index));
                let marker = self.push_call("updateEveryFrame", vec![marker]);
                let replacement = self.lower_value(arg)?;
                value = self.push_call("stringReplace", vec![value, marker, replacement]);
            }
            value
        } else {
            let mut custom_args = vec![text];
            custom_args.extend(self.lower_values(format_args.iter().copied())?);
            self.push_call("customString", custom_args)
        };
        let helper_id = *self.globals.get(TRANSLATION_HELPER_NAME).ok_or_else(|| {
            IntegrationError::new(
                "translations-invalid",
                "translation helper variable was not allocated",
                span,
            )
        })?;
        let helper = self.push_value(Value::GlobalVariable(self.global_names[helper_id].clone()));
        let translated = self.push_call("stringSplit", vec![custom, helper]);
        if name == "___" {
            return Ok(translated);
        }
        if name == "_"
            && self
                .hir
                .preprocessing
                .directives
                .iter()
                .any(|directive| directive.name == "translateWithPlayerVar")
        {
            let variable = *self
                .players
                .get("__languageIndex__")
                .expect("translation player variable is allocated");
            let player = self.push_call("localPlayer", Vec::new());
            let index = self.push_value(Value::PlayerVariable {
                player,
                variable: self.player_names[variable].clone(),
            });
            return Ok(self.push_call("valueInArray", vec![translated, index]));
        }
        Ok(self.select_translation(translated))
    }

    fn select_translation(&mut self, values: ValueId) -> ValueId {
        let helper_id = *self
            .globals
            .get(TRANSLATION_HELPER_NAME)
            .expect("translation helper variable is allocated");
        let helper = self.push_value(Value::GlobalVariable(self.global_names[helper_id].clone()));
        let color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let empty_array = self.push_call("emptyArray", Vec::new());
        let color = self.push_call("stringSplit", vec![color, empty_array]);
        let index = self.push_call("indexOfArrayValue", vec![helper, color]);
        let index = self.push_call("absoluteValue", vec![index]);
        self.push_call("valueInArray", vec![values, index])
    }

    pub(super) fn translation_language_index(
        &mut self,
        translations: &hir::TranslationState,
    ) -> Result<ValueId, IntegrationError> {
        let helper = self.lower_translation_helper(translations)?;
        let color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let empty_array = self.push_call("emptyArray", Vec::new());
        let color = self.push_call("stringSplit", vec![color, empty_array]);
        Ok(self.push_call("indexOfArrayValue", vec![helper, color]))
    }

    pub(in crate::compiler) fn translation_files(&self) -> Vec<(String, String)> {
        let Some(translations) = self.hir.preprocessing.translations.as_ref() else {
            return Vec::new();
        };
        let keep_unused = self
            .hir
            .preprocessing
            .directives
            .iter()
            .any(|directive| directive.name == "keepUnusedTranslations");
        translations
            .languages
            .iter()
            .skip(1)
            .map(|language| {
                let mut keys = self.translation_uses.clone();
                if keep_unused {
                    keys.extend(
                        translations
                            .entries
                            .iter()
                            .map(|entry| (entry.msgid.clone(), entry.context.clone())),
                    );
                }
                keys.sort();
                keys.dedup();
                let mut output = String::from(
                    "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n",
                );
                output.push_str(&format!("\"Language: {language}\\n\"\n\n"));
                for (msgid, context) in keys {
                    if let Some(ref context) = context {
                        output.push_str(&format!(
                            "msgctxt {}\n",
                            serde_json::to_string(&context).unwrap()
                        ));
                    }
                    let translated = translations
                        .entries
                        .iter()
                        .find(|entry| {
                            entry.msgid == msgid && entry.context.as_deref() == context.as_deref()
                        })
                        .and_then(|entry| entry.translations.get(language))
                        .cloned()
                        .unwrap_or_default();
                    output.push_str(&format!(
                        "msgid {}\n",
                        serde_json::to_string(&msgid).unwrap()
                    ));
                    output.push_str(&format!(
                        "msgstr {}\n\n",
                        serde_json::to_string(&translated).unwrap()
                    ));
                }
                (language.clone(), output)
            })
            .collect()
    }

    pub(super) fn lower_debug(
        &mut self,
        expr: &Expr,
        debug_source: Option<&str>,
    ) -> Result<ActionId, IntegrationError> {
        let argument_span = expr.span().copied();
        let value = self.lower_text_value(expr)?;
        let array_text = if self.debug_value_is_array(value) {
            self.lower_debug_array_text(value, 6)
        } else {
            value
        };
        let debug_label_text = debug_source
            .map(str::to_string)
            .unwrap_or_else(|| debug_expr_text(expr));
        let debug_label = canonical_debug_text(&debug_label_text);
        let debug_prefix = format!("{debug_label}\u{2028}= {{0}}");
        let inline_padding = 128 - debug_prefix.chars().count() - "{1}".chars().count();
        let padding_text = self.push_value(Value::String(" ".repeat(170 - inline_padding)));
        let padding = self.push_call("customString", vec![padding_text]);
        let debug_label = self.push_value(Value::String(format!(
            "{debug_prefix}{}{{1}}",
            " ".repeat(inline_padding)
        )));
        let text = self.push_call("customString", vec![debug_label, array_text, padding]);
        let all_players = self.lower_all_players();
        let null_value = self.push_value(Value::Null);
        let null_value_2 = self.push_value(Value::Null);
        let null_value_3 = self.push_value(Value::Null);
        let null_value_4 = self.push_value(Value::Null);
        let hud_position = self.push_value(Value::Enum {
            value_type: "HudPosition".to_string(),
            value: "LEFT".to_string(),
        });
        let sort_order = self.push_number(-9999.0);
        let color = self.push_value(Value::Enum {
            value_type: "Color".to_string(),
            value: "WHITE".to_string(),
        });
        let reevaluation = self.push_value(Value::Enum {
            value_type: "HudReeval".to_string(),
            value: "VISIBILITY_SORT_ORDER_STRING_AND_COLOR".to_string(),
        });
        let visibility = self.push_value(Value::Enum {
            value_type: "SpecVisibility".to_string(),
            value: "DEFAULT".to_string(),
        });
        let args = self.normalize_contextual_arguments(
            "createHudText",
            vec![
                all_players,
                null_value,
                text,
                null_value_2,
                hud_position,
                sort_order,
                null_value_3,
                color,
                null_value_4,
                reevaluation,
                visibility,
            ],
        );
        Ok(self.push_call_action_with_spans(
            "createHudText",
            &args,
            [
                None,
                None,
                argument_span,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        ))
    }

    pub(super) fn lower_print(
        &mut self,
        expr: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ActionId, IntegrationError> {
        let argument_span = expr.span().copied();
        let empty_string = matches!(expr, Expr::String { value, .. } if value.is_empty());
        let value = self.lower_value(expr)?;
        let value = if empty_string {
            self.push_value(Value::Null)
        } else {
            value
        };
        let padding_text = self.push_value(Value::String(" ".repeat(45)));
        let padding = self.push_call("customString", vec![padding_text]);
        let body_text = self.push_value(Value::String(format!("{}{{0}}", " ".repeat(125))));
        let body = self.push_call("customString", vec![body_text, padding]);
        let all_players = self.lower_all_players();
        let null_value = self.push_value(Value::Null);
        let null_value_2 = self.push_value(Value::Null);
        let null_value_3 = self.push_value(Value::Null);
        let hud_position = self.push_value(Value::Enum {
            value_type: "HudPosition".to_string(),
            value: "LEFT".to_string(),
        });
        let sort_order = self.push_number(-9999.0);
        let color = if empty_string {
            self.push_value(Value::Null)
        } else {
            self.push_value(Value::Enum {
                value_type: "Color".to_string(),
                value: "ORANGE".to_string(),
            })
        };
        let reevaluation = self.push_value(Value::Enum {
            value_type: "HudReeval".to_string(),
            value: "VISIBILITY_AND_STRING".to_string(),
        });
        let visibility = self.push_value(Value::Enum {
            value_type: "SpecVisibility".to_string(),
            value: "DEFAULT".to_string(),
        });
        let mut args = self.normalize_contextual_arguments(
            "createHudText",
            vec![
                all_players,
                value,
                body,
                null_value,
                hud_position,
                sort_order,
                color,
                null_value_2,
                null_value_3,
                reevaluation,
                visibility,
            ],
        );
        self.apply_replacements_to_values("createHudText", &mut args, span);
        Ok(self.push_call_action_with_spans(
            "createHudText",
            &args,
            [
                None,
                argument_span,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        ))
    }

    pub(super) fn lower_debug_array_text(&mut self, value: ValueId, max_length: usize) -> ValueId {
        macro_rules! call {
            ($name:literal $(, $arg:expr)* $(,)?) => {{
                let args = vec![$($arg),*];
                self.push_call($name, args)
            }};
        }

        let current_count = call!("countOf", call!("currentArrayElement"));
        let is_single = call!(
            "==",
            call!("countOf", call!("currentArrayElement")),
            self.push_number(1.0)
        );
        let is_empty = call!("==", call!("currentArrayElement"), call!("emptyArray"));
        let not_null = call!(
            "!=",
            call!("currentArrayElement"),
            self.push_value(Value::Null)
        );
        let has_empty_array = call!("and", is_empty, not_null);
        let brackets = call!("or", is_single, has_empty_array);
        let first_element = call!(
            "customString",
            self.push_value(Value::String("[{0}]".to_string())),
            call!("currentArrayElement"),
        );
        let many_elements = call!(
            "customString",
            self.push_value(Value::String("[{0}, …+{1}]".to_string())),
            call!("currentArrayElement"),
            call!(
                "subtract",
                call!("countOf", call!("currentArrayElement")),
                self.push_number(1.0),
            ),
        );
        let element_text = call!(
            "ifThenElse",
            brackets,
            first_element,
            call!(
                "ifThenElse",
                current_count,
                many_elements,
                call!("currentArrayElement"),
            ),
        );
        let mapped_elements = call!("mappedArray", value, element_text,);
        let mapped_input = call!("array", mapped_elements);
        let current_array = call!("currentArrayElement");
        let actual_array = call!(
            "or",
            call!("countOf", current_array),
            call!(
                "and",
                call!("==", call!("currentArrayElement"), call!("emptyArray")),
                call!(
                    "!=",
                    call!("currentArrayElement"),
                    self.push_value(Value::Null)
                ),
            ),
        );
        let empty_length = call!(
            "ifThenElse",
            call!(
                "and",
                call!("not", call!("countOf", call!("currentArrayElement"))),
                call!("!=", call!("currentArrayElement"), call!("emptyArray"),),
            ),
            self.push_number(3.0),
            call!(
                "multiply",
                call!("countOf", call!("currentArrayElement")),
                self.push_number(3.0),
            ),
        );
        let x = call!(
            "appendToArray",
            call!("appendToArray", actual_array, empty_length),
            current_array,
        );
        let x_input = call!("mappedArray", mapped_input, x);
        let x_length = |this: &mut Self| {
            let current = this.push_call("currentArrayElement", Vec::new());
            let index = this.push_number(1.0);
            this.push_call("valueInArray", vec![current, index])
        };
        let x_value = |this: &mut Self, index: f64| {
            let current = this.push_call("currentArrayElement", Vec::new());
            let index_value = this.push_number(index);
            this.push_call("valueInArray", vec![current, index_value])
        };
        let first = call!("firstOf", call!("currentArrayElement"));
        // The reference splits the flattened `{0}, {1}, …` display format
        // into `customString` chunks of at most three slots: outer chunks
        // hold two elements and pass the remaining format through `{2}`,
        // and the innermost chunk keeps the last two or three elements.
        let tail_length = match max_length {
            0..=3 => max_length,
            length if length % 2 == 0 => 2,
            _ => 3,
        };
        let tail = format!(
            "{}…\u{0001}",
            (0..tail_length)
                .map(|index| format!("{{{index}}}, "))
                .collect::<String>()
        );
        let mut args = vec![self.push_value(Value::String(tail))];
        for index in 0..tail_length {
            args.push(x_value(self, (max_length - tail_length + index + 2) as f64));
        }
        let mut array_head = self.push_call("customString", args);
        for chunk in (0..(max_length - tail_length) / 2).rev() {
            array_head = call!(
                "customString",
                self.push_value(Value::String("{0}, {1}, {2}".to_string())),
                x_value(self, (chunk * 2 + 2) as f64),
                x_value(self, (chunk * 2 + 3) as f64),
                array_head,
            );
        }
        let placeholder_text = format!(
            "{}\u{2026}\u{0001}",
            (0..max_length).map(|_| "0, ").collect::<String>()
        );
        let placeholder = call!(
            "customString",
            self.push_value(Value::String(placeholder_text.clone())),
        );
        let length_for_slice = x_length(self);
        let end_length_for_slice = x_length(self);
        let start = self.push_number(
            (placeholder_text.chars().count() as isize - 4 - 3 * max_length as isize) as f64,
        );
        let end = self.push_number((max_length * 3 + 4) as f64);
        let slice = call!(
            "stringSlice",
            placeholder,
            call!("add", start, length_for_slice),
            call!("subtract", end, end_length_for_slice,),
        );
        let replaced = call!("stringReplace", array_head, slice, call!("emptyArray"),);
        let length_for_compare = x_length(self);
        let length_for_divide = x_length(self);
        let plus = call!(
            "ifThenElse",
            call!(
                ">",
                length_for_compare,
                self.push_number((max_length * 3) as f64),
            ),
            call!(
                "customString",
                self.push_value(Value::String("+{0}".to_string())),
                call!(
                    "subtract",
                    call!("divide", length_for_divide, self.push_number(3.0)),
                    self.push_number(max_length as f64),
                ),
            ),
            call!("emptyArray"),
        );
        let formatted_array = call!(
            "customString",
            self.push_value(Value::String("[{0}{1}]".to_string())),
            replaced,
            plus,
        );
        let current_for_split = call!("currentArrayElement");
        let rendered = call!(
            "ifThenElse",
            first,
            formatted_array,
            call!(
                "stringSplit",
                call!("valueInArray", current_for_split, self.push_number(2.0)),
                call!("emptyArray"),
            ),
        );
        call!("mappedArray", x_input, rendered)
    }

    pub(super) fn lower_text_value(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        let value = self.lower_value(expr)?;
        let Value::Call { name, args } = self.value(value) else {
            return Ok(value);
        };
        if name == "customString" && args.len() == 1 {
            Ok(args[0])
        } else {
            Ok(value)
        }
    }

    fn debug_value_is_array(&self, value: ValueId) -> bool {
        match self.value(value) {
            Value::GlobalVariable(_) | Value::Array(_) => true,
            Value::Call { name, .. } if matches!(name.as_str(), "array" | "emptyArray") => true,
            Value::Call { name, .. } => self
                .compiler
                .catalog
                .entry(Kind::Value, name)
                .and_then(|entry| entry.return_type())
                .is_some_and(|return_type| {
                    return_type.split('|').any(|part| part.trim() == "Array")
                }),
            _ => false,
        }
    }

    pub(super) fn value_is_known_player(&self, value: ValueId) -> bool {
        match self.value(value) {
            Value::EventPlayer => true,
            Value::Call { name, .. } => self
                .compiler
                .catalog
                .entry(Kind::Value, name)
                .and_then(|entry| entry.return_type())
                .is_some_and(|return_type| {
                    return_type.split('|').any(|part| part.trim() == "Player")
                }),
            _ => false,
        }
    }
}
