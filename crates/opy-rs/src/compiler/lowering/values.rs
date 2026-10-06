use super::*;

impl<'a> Lowering<'a> {
    pub(super) fn lower_values<'expr>(
        &mut self,
        expressions: impl IntoIterator<Item = &'expr Expr>,
    ) -> Result<Vec<ValueId>, IntegrationError> {
        expressions
            .into_iter()
            .map(|expr| self.lower_value(expr))
            .collect()
    }

    pub(super) fn lower_value(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        let optimization = self.optimization_state_at(expr.span());
        let previous = std::mem::replace(
            &mut self.optimization_mark,
            optimization.enabled.then_some(optimization.strict),
        );
        let result = self.lower_value_unoptimized(expr);
        self.optimization_mark = previous;
        result
    }

    fn lower_value_unoptimized(&mut self, expr: &Expr) -> Result<ValueId, IntegrationError> {
        let span = expr.span().copied();
        let optimization = self.optimization_state_at(span.as_ref());
        if optimization.enabled
            && !optimization.strict
            && (matches!(expr, Expr::Binary { .. } | Expr::Unary { .. })
                || matches!(expr, Expr::Call { name, .. } if matches!(name.as_str(), "len" | "countOf")))
        {
            let bindings = HashMap::new();
            let mut stack = Vec::new();
            if let Some(value) =
                crate::compile_time::evaluate(expr, &self.constants, &bindings, &mut stack)
            {
                match value {
                    crate::compile_time::Value::Number(value)
                        if value.is_finite()
                            && value.abs()
                                <= crate::compiler::operator_optimization::NUMBER_LIMIT =>
                    {
                        return Ok(self.push_number(value));
                    }
                    crate::compile_time::Value::String(value) => {
                        return Ok(self.lower_custom_string(value));
                    }
                    crate::compile_time::Value::Bool(value) => {
                        return Ok(self.push_value(Value::Bool(value)));
                    }
                    crate::compile_time::Value::Array(_)
                    | crate::compile_time::Value::Object(_)
                    | crate::compile_time::Value::Number(_) => {}
                }
            }
        }
        let value = match expr {
            Expr::Number { value, .. } => Value::Number(*value),
            Expr::String { value, .. } => {
                return Ok(self.lower_custom_string(value.clone()));
            }
            Expr::Bool { value, .. } => Value::Bool(*value),
            Expr::Null { .. } => Value::Null,
            Expr::Local { name, .. } => {
                let binding = self.array_bindings.iter().rev().find(|binding| {
                    binding.element == *name || binding.index.as_deref() == Some(name)
                });
                match binding {
                    Some(binding) if binding.element == *name => {
                        return Ok(self.push_call("currentArrayElement", Vec::new()));
                    }
                    Some(_) => return Ok(self.push_call("currentArrayIndex", Vec::new())),
                    None => {
                        return Err(self.unsupported(
                            format!("local '{name}' is not inside a supported array callback"),
                            span,
                        ));
                    }
                }
            }
            Expr::Type { .. } => {
                return Err(self.unsupported(
                    "type expressions are only valid as createWorkshopSetting type arguments",
                    span,
                ));
            }
            Expr::GlobalVar { name, .. } => {
                let id = self.global_variable_id(name, span)?;
                Value::GlobalVariable(self.global_names[id].clone())
            }
            Expr::PlayerVar { player, name, .. } => {
                let player = self.lower_value(player)?;
                let id = self.player_variable_id(name, span)?;
                Value::PlayerVariable {
                    player,
                    variable: self.player_names[id].clone(),
                }
            }
            Expr::EventPlayer { .. } => Value::EventPlayer,
            Expr::HostPlayer { .. } => Value::Call {
                name: "hostPlayer".to_string(),
                args: Vec::new(),
            },
            Expr::Enum {
                value_type,
                value,
                literal,
                ..
            } => {
                let value = match (value_type.as_str(), value.as_str()) {
                    ("Clipping", "NONE") => "DO_NOT_CLIP",
                    ("Clipping", "SURFACES") => "CLIP_AGAINST_SURFACES",
                    _ => value,
                };
                if self
                    .compiler
                    .catalog
                    .enum_spelling(value_type, &Locale::new("en-US"), value)
                    .is_none()
                {
                    return Err(self.unsupported(
                        format!("unknown catalog enum member '{value_type}.{value}'"),
                        span,
                    ));
                }
                let member = Value::Enum {
                    value_type: value_type.clone(),
                    value: value.to_string(),
                };
                // A `*Literal` member emits the bare display-name lookup,
                // without the canonical wrapper (`Game Mode(...)`, `Hero(...)`).
                if *literal {
                    member
                } else if value_type == "Gamemode" {
                    let member = self.push_value(member);
                    Value::Call {
                        name: "gameMode".to_string(),
                        args: vec![member],
                    }
                } else {
                    member
                }
            }
            Expr::Array { elements, .. } => {
                let elements = self.lower_values(elements)?;
                return Ok(self.lower_array(elements));
            }
            Expr::Vector { x, y, z, .. } => {
                let x = self.lower_value(x)?;
                let y = self.lower_value(y)?;
                let z = self.lower_value(z)?;
                if let Some(member) = self.canonical_vector_member(x, y, z) {
                    Value::Enum {
                        value_type: "Vector".to_string(),
                        value: member.to_string(),
                    }
                } else {
                    Value::Call {
                        name: "vector".to_string(),
                        args: self.value_args(&[x, y, z]),
                    }
                }
            }
            Expr::Constant { name, .. } => {
                let const_expr = *self
                    .constants
                    .get(name)
                    .ok_or_else(|| self.unsupported(format!("unknown constant '{name}'"), span))?;
                // The reference substitutes the definition at the use site, so
                // the whole subtree optimizes under the caller's state rather
                // than the definition's spans.
                let override_state = self.optimization_state_at(span.as_ref());
                let previous = self.optimization_override.replace(override_state);
                let value = self.lower_value(const_expr);
                self.optimization_override = previous;
                return value;
            }
            Expr::Index { array, index, .. } => {
                if let Expr::Dict { entries, .. } = array.as_ref()
                    && is_literal_key(index)
                    && entries.iter().all(|entry| is_literal_key(&entry.key))
                {
                    if let Some(value) = entries
                        .iter()
                        .find(|entry| literal_key_matches(&entry.key, index))
                        .map(|entry| &entry.value)
                    {
                        return self.lower_value(value);
                    }
                    return Ok(self.push_value(Value::Null));
                }
                // The pinned OverPy oracle lowers a literal zero-index read
                // (`arr[0]`, `arr[0.0]`) to `firstOf(arr)`; non-zero indexes
                // and indexed writes keep the indexed forms.
                if matches!(index.as_ref(), Expr::Number { value, .. } if *value == 0.0) {
                    let array = self.lower_value(array)?;
                    Value::Call {
                        name: "firstOf".to_string(),
                        args: self.value_args(&[array]),
                    }
                } else {
                    let array = self.lower_value(array)?;
                    let index = self.lower_value(index)?;
                    Value::Call {
                        name: "valueInArray".to_string(),
                        args: self.value_args(&[array, index]),
                    }
                }
            }
            Expr::Format { text, args, .. } => {
                let (format_text, dynamic_args) = self.fold_format_constants(text, args);
                if dynamic_args.is_empty() {
                    let value = format_text;
                    return Ok(self.lower_custom_string(value));
                }
                if dynamic_args.len() <= 3 {
                    let text_node = self.push_value(Value::String(format_text));
                    let mut call_args = vec![text_node];
                    for arg in dynamic_args {
                        let arg = self.lower_value(arg)?;
                        call_args.push(arg);
                    }
                    Value::Call {
                        name: "customString".to_string(),
                        args: call_args,
                    }
                } else {
                    let chunks = split_format_chunks(&format_text, dynamic_args.len()).ok_or_else(|| {
                        self.unsupported(
                            "format strings with more than three replacements require sequential placeholders",
                            span,
                        )
                    })?;
                    let lowered_args = dynamic_args.iter().copied();
                    let lowered_args = self.lower_values(lowered_args)?;
                    let mut parts = Vec::with_capacity(chunks.len());
                    for (chunk, indices) in chunks {
                        let text = self.push_value(Value::String(chunk));
                        let mut call_args = vec![text];
                        call_args.extend(indices.into_iter().map(|index| lowered_args[index]));
                        let call_args =
                            self.normalize_contextual_arguments("customString", call_args);
                        parts.push(self.push_value(Value::Call {
                            name: "customString".to_string(),
                            args: call_args,
                        }));
                    }
                    let separator = self.push_value(Value::String("{0}{1}".to_string()));
                    let mut value = parts[0];
                    for part in parts.into_iter().skip(1) {
                        value = self.push_call("customString", vec![separator, value, part]);
                    }
                    return Ok(value);
                }
            }
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                ..
            } => Value::Call {
                name: "ifThenElse".to_string(),
                args: {
                    let condition = self.lower_value(condition)?;
                    let then_value = self.lower_value(then_value)?;
                    let else_value = self.lower_value(else_value)?;
                    self.value_args(&[condition, then_value, else_value])
                },
            },
            Expr::Binary {
                op, left, right, ..
            } => {
                if self.optimization_state_at(span.as_ref()).enabled
                    && matches!(op.as_str(), "in" | "not in")
                {
                    if let Expr::Array { elements, .. } = right.as_ref() {
                        if elements
                            .iter()
                            .any(|elem| literal_key_matches(elem, left.as_ref()))
                        {
                            return Ok(self.push_value(Value::Bool(op == "in")));
                        }
                        let strict = self.strict_optimization_active(expr);
                        if is_membership_literal(left.as_ref(), strict)
                            && elements
                                .iter()
                                .all(|elem| is_membership_literal(elem, strict))
                        {
                            return Ok(self.push_value(Value::Bool(op == "not in")));
                        }
                    }
                }
                if op == "==" && self.optimization_state_at(span.as_ref()).enabled {
                    if let Some(value) = self.lower_current_map_equality(left, right) {
                        return Ok(value);
                    }
                }
                let left = self.lower_value(left)?;
                let right = self.lower_value(right)?;
                if self.optimization_state_at(span.as_ref()).enabled
                    && let Some(value) = self.fold_numeric_binary(op, left, right)
                {
                    Value::Number(value)
                } else {
                    if op == "in" {
                        Value::Call {
                            name: "arrayContains".to_string(),
                            args: self.value_args(&[right, left]),
                        }
                    } else if op == "not in" {
                        let contains = self.push_call("arrayContains", vec![right, left]);
                        Value::Call {
                            name: "not".to_string(),
                            args: self.value_args(&[contains]),
                        }
                    } else {
                        let name = match op.as_str() {
                            "==" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or" => op,
                            "+" => "add",
                            "-" => "subtract",
                            "*" => "multiply",
                            "/" => "divide",
                            "%" => "modulo",
                            "**" => "raiseToPower",
                            _ => {
                                return Err(self.unsupported(
                                    format!(
                                        "binary operator '{op}' is not currently representable in canonical WIR"
                                    ),
                                    span,
                                ));
                            }
                        };
                        Value::Call {
                            name: name.to_string(),
                            args: self.value_args(&[left, right]),
                        }
                    }
                }
            }
            Expr::Unary { op, operand, .. } => match op.as_str() {
                "not" => {
                    // The pinned OverPy 9.7.10 oracle lowers `not (a == b)`
                    // to the negated comparison (`a != b`), flipping every
                    // ordering comparison; `in` membership stays wrapped in
                    // `not`. Mirror that observable lowering.
                    if let Expr::Binary {
                        op: comparison,
                        left,
                        right,
                        ..
                    } = operand.as_ref()
                    {
                        if let Some(negated) = negated_comparison(comparison) {
                            let left = self.lower_value(left)?;
                            let right = self.lower_value(right)?;
                            Value::Call {
                                name: negated.to_string(),
                                args: self.value_args(&[left, right]),
                            }
                        } else {
                            let operand = self.lower_value(operand)?;
                            Value::Call {
                                name: "not".to_string(),
                                args: self.value_args(&[operand]),
                            }
                        }
                    } else {
                        let operand = self.lower_value(operand)?;
                        Value::Call {
                            name: "not".to_string(),
                            args: self.value_args(&[operand]),
                        }
                    }
                }
                "-" => {
                    let operand = self.lower_value(operand)?;
                    if let Value::Number(number) = self.value(operand) {
                        Value::Number(-number)
                    } else {
                        Value::Call {
                            name: "-".to_string(),
                            args: self.value_args(&[operand]),
                        }
                    }
                }
                "+" => return self.lower_value(operand),
                _ => {
                    return Err(self.unsupported(
                        format!(
                            "unary operator '{op}' is not currently representable in canonical WIR"
                        ),
                        span,
                    ));
                }
            },
            Expr::Call { name, args, .. } => {
                if matches!(name.as_str(), "_" | "__" | "___") {
                    return self.lower_translation(name, args, span);
                }
                if name == "createWorkshopSetting" {
                    return self.lower_workshop_setting(args, span);
                }
                if name == "buttonToString" {
                    let [button] = args.as_slice() else {
                        return Err(self.unsupported("buttonToString requires one button", span));
                    };
                    let button = self.lower_value(button)?;
                    // The reference expands the `buttonToString` macro
                    // syntactically and emits the expansion unfolded even
                    // under `#!optimizeForSize`, so the synthesized nodes
                    // stay outside the optimization mark.
                    let optimization = self.optimization_mark.take();
                    let bindings = self.push_call("inputBindingString", vec![button]);
                    // The reference maps over the binding String directly,
                    // which is not canonical (`mappedArray` takes `Array`);
                    // the expansion needs the whole string as the sole
                    // element, so it wraps it in `Array` — the canonical
                    // form of the same formula (approved exception,
                    // docs/architecture/language-core.md).
                    let bindings = self.lower_array(vec![bindings]);
                    let element = self.push_call("currentArrayElement", Vec::new());
                    let labels = self.push_value(Value::String(
                        "{0}(0.00, 1.00, 0.00)[{0}](0.00, 1.00, 0.00)[SHIFT](0.00, 1.00, 0.00)[CTRL](0.00, 1.00, 0.00)[ALT]"
                            .to_string(),
                    ));
                    let padded = self.push_call("customString", vec![labels, element]);
                    let up = self.push_value(Value::Enum {
                        value_type: "Vector".to_string(),
                        value: "UP".to_string(),
                    });
                    let first_up = self.push_call("firstOf", vec![up]);
                    let segments = self.push_call("stringSplit", vec![padded, first_up]);
                    let element = self.push_call("currentArrayElement", Vec::new());
                    let length_text =
                        self.push_value(Value::String("\\{0}{0}{0}{0}{0}{0}{0}".to_string()));
                    let length_text = self.push_call("customString", vec![length_text, element]);
                    let length = self.push_call("strLen", vec![length_text]);
                    let seven = self.push_number(7.0);
                    let remainder = self.push_call("modulo", vec![length, seven]);
                    let one = self.push_number(1.0);
                    let is_texture = self.push_call("==", vec![remainder, one]);
                    let keys = self.push_value(Value::String(
                        "\u{ec47}0\u{ec47}0LSHIFT0LCONTROL0LALT".to_string(),
                    ));
                    let keys = self.push_call("customString", vec![keys]);
                    let null = self.push_value(Value::Null);
                    let first_null = self.push_call("firstOf", vec![null]);
                    let keys = self.push_call("stringSplit", vec![keys, first_null]);
                    let element = self.push_call("currentArrayElement", Vec::new());
                    let key_index = self.push_call("indexOfArrayValue", vec![keys, element]);
                    let key_index = self.push_call("absoluteValue", vec![key_index]);
                    let index = self.push_call("and", vec![is_texture, key_index]);
                    let segment = self.push_call("valueInArray", vec![segments, index]);
                    let result = self.push_call("mappedArray", vec![bindings, segment]);
                    self.optimization_mark = optimization;
                    return Ok(result);
                }
                if matches!(
                    name.as_str(),
                    "getRealClosestPlayer"
                        | "getRealClosestPlayers"
                        | "getRealFarthestPlayer"
                        | "getRealFarthestPlayers"
                ) {
                    let [center, team] = args.as_slice() else {
                        return Err(
                            self.unsupported(format!("{name} requires center and team"), span)
                        );
                    };
                    let center = self.lower_value(center)?;
                    let team = self.lower_value(team)?;
                    let players = self.push_call("getLivingPlayers", vec![team]);
                    let current = self.push_call("currentArrayElement", Vec::new());
                    let spawned = self.push_call("hasSpawned", vec![current]);
                    let players = self.push_call("filteredArray", vec![players, spawned]);
                    let distance = self.push_call("distance", vec![current, center]);
                    let key = if matches!(
                        name.as_str(),
                        "getRealFarthestPlayer" | "getRealFarthestPlayers"
                    ) {
                        let negative_one = self.push_number(-1.0);
                        self.push_call("multiply", vec![negative_one, distance])
                    } else {
                        distance
                    };
                    let sorted = self.push_call("sortedArray", vec![players, key]);
                    return if matches!(
                        name.as_str(),
                        "getRealClosestPlayer" | "getRealFarthestPlayer"
                    ) {
                        Ok(self.push_call("firstOf", vec![sorted]))
                    } else {
                        Ok(sorted)
                    };
                }
                if name == "getRealPlayersInRadius" {
                    let lowered = self.lower_values(args)?;
                    let players = self.push_call("getPlayersInRadius", lowered);
                    let current = self.push_call("currentArrayElement", Vec::new());
                    let alive = self.push_call("isAlive", vec![current]);
                    let spawned = self.push_call("hasSpawned", vec![current]);
                    let condition = self.push_call("and", vec![alive, spawned]);
                    return Ok(self.push_call("filteredArray", vec![players, condition]));
                }
                if name == "lineIntersectsSphere" {
                    let [line_start, line_direction, sphere_center, sphere_radius] =
                        args.as_slice()
                    else {
                        return Err(
                            self.unsupported("lineIntersectsSphere requires four arguments", span)
                        );
                    };
                    let line_start = self.lower_value(line_start)?;
                    let line_direction = self.lower_value(line_direction)?;
                    let sphere_center = self.lower_value(sphere_center)?;
                    let sphere_radius = self.lower_value(sphere_radius)?;
                    let angle =
                        self.push_call("angleBetweenVectors", vec![line_start, line_direction]);
                    let distance = self.push_call("distance", vec![line_start, sphere_center]);
                    let ratio = self.push_call("divide", vec![sphere_radius, distance]);
                    let limit = self.push_call("asinDeg", vec![ratio]);
                    return Ok(self.push_call("<=", vec![angle, limit]));
                }
                if name == "arrayToString" {
                    let (array, max_length) = match args.as_slice() {
                        [array] => (array, 12),
                        [array, Expr::Number { value, .. }] => {
                            if !value.is_finite() || *value < 0.0 || value.fract() != 0.0 {
                                return Err(self.unsupported(
                                    "arrayToString maxLength must be a non-negative integer literal",
                                    span,
                                ));
                            }
                            (array, (*value).min(1000.0) as usize)
                        }
                        _ => return Err(self.unsupported("arrayToString requires an array", span)),
                    };
                    let array = self.lower_value(array)?;
                    return Ok(self.lower_debug_array_text(array, max_length));
                }
                if matches!(name.as_str(), "decompressNumbers" | "decompressVectors") {
                    let [text] = args.as_slice() else {
                        return Err(self.unsupported(format!("{name} requires one string"), span));
                    };
                    return self.lower_decompression(text, name == "decompressVectors");
                }
                if name == "strVisualLength" {
                    let [Expr::String { value, .. }] = args.as_slice() else {
                        return Err(
                            self.unsupported("strVisualLength requires one literal string", span)
                        );
                    };
                    let width = value.chars().map(blizzard_global::width).sum::<i32>();
                    return Ok(self.push_number(width as f64));
                }
                if name == "spacesForLength" {
                    let [Expr::Number { value, .. }] = args.as_slice() else {
                        return Err(
                            self.unsupported("spacesForLength requires one literal number", span)
                        );
                    };
                    if !value.is_finite() || *value < 0.0 || value.fract() != 0.0 {
                        return Err(self.unsupported(
                            "spacesForLength requires a non-negative integer literal",
                            span,
                        ));
                    }
                    return Ok(self.lower_custom_string(blizzard_global::spaces(*value as i32)));
                }
                if name == "spacesForString" {
                    let [Expr::String { value, .. }] = args.as_slice() else {
                        if let [
                            Expr::Call {
                                name: translation,
                                args: translation_args,
                                ..
                            },
                        ] = args.as_slice()
                            && matches!(translation.as_str(), "_" | "__" | "___")
                            && let Some(text) = translation_args.last()
                            && let Expr::String {
                                value,
                                span: text_span,
                            } = text
                        {
                            let replacement = Expr::String {
                                value: blizzard_global::spaces(
                                    value.chars().map(blizzard_global::width).sum(),
                                ),
                                span: *text_span,
                            };
                            let mut translated_args = translation_args.clone();
                            *translated_args.last_mut().expect("translation text exists") =
                                replacement;
                            return self.lower_value(&Expr::Call {
                                name: translation.clone(),
                                args: translated_args,
                                debug_source: None,
                                span,
                            });
                        }
                        return Err(
                            self.unsupported("spacesForString requires one literal string", span)
                        );
                    };
                    return Ok(self.lower_custom_string(blizzard_global::spaces(
                        value.chars().map(blizzard_global::width).sum(),
                    )));
                }
                if matches!(name.as_str(), "hsl" | "hsla") {
                    let (hue, saturation, lightness, alpha) = match args.as_slice() {
                        [hue, saturation, lightness] => (hue, saturation, lightness, None),
                        [hue, saturation, lightness, alpha] => {
                            (hue, saturation, lightness, Some(alpha))
                        }
                        _ => {
                            return Err(
                                self.unsupported("hsl requires three or four arguments", span)
                            );
                        }
                    };
                    // The reference checks constant-folded ranges first, then
                    // random inputs, each reported at the offending argument.
                    // Its fold runs at parse time, so the check must see each
                    // argument's optimized value, not only lowering-time folds.
                    let hue = self.lower_value(hue)?;
                    let saturation = self.lower_value(saturation)?;
                    let lightness = self.lower_value(lightness)?;
                    let alpha = match alpha {
                        Some(alpha) => self.lower_value(alpha)?,
                        None => self.push_number(255.0),
                    };
                    for (index, (argument, label, low, high)) in [
                        (hue, "Hue", 0.0, 360.0),
                        (saturation, "Saturation", 0.0, 1.0),
                        (lightness, "Lightness", 0.0, 1.0),
                        (alpha, "Alpha", 0.0, 255.0),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if let workshop_rs::Value::Number(value) =
                            self.materialize_value_inner(argument)
                            && !(low..=high).contains(&value)
                        {
                            return Err(self.unsupported(
                                format!("{label} must be between {low} and {high}"),
                                args.get(index).and_then(|expr| expr.span().copied()),
                            ));
                        }
                    }
                    for expr in args {
                        if hir::visit::contains_random(expr) {
                            return Err(self.unsupported(
                                "Cannot use random functions in hsl() or hsla()",
                                expr.span().copied(),
                            ));
                        }
                    }
                    let one = self.push_number(1.0);
                    let thirty = self.push_number(30.0);
                    let hue_thirtieths = self.push_call("divide", vec![hue, thirty]);
                    let lightness_complement = self.push_call("subtract", vec![one, lightness]);
                    let lightness_limit =
                        self.push_call("min", vec![lightness, lightness_complement]);
                    let channel = |this: &mut Self, offset: f64| {
                        let offset = this.push_number(offset);
                        let phase = this.push_call("add", vec![offset, hue_thirtieths]);
                        let twelve = this.push_number(12.0);
                        let phase = this.push_call("modulo", vec![phase, twelve]);
                        let three = this.push_number(3.0);
                        let lower = this.push_call("subtract", vec![phase, three]);
                        let nine = this.push_number(9.0);
                        let upper = this.push_call("subtract", vec![nine, phase]);
                        let inner = this.push_call("min", vec![lower, upper]);
                        let one = this.push_number(1.0);
                        let inner = this.push_call("min", vec![inner, one]);
                        let negative_one = this.push_number(-1.0);
                        let clamped = this.push_call("max", vec![inner, negative_one]);
                        let saturation_limit =
                            this.push_call("multiply", vec![saturation, lightness_limit]);
                        let adjustment =
                            this.push_call("multiply", vec![saturation_limit, clamped]);
                        let value = this.push_call("subtract", vec![lightness, adjustment]);
                        let scale = this.push_number(255.0);
                        this.push_call("multiply", vec![value, scale])
                    };
                    let red = channel(self, 0.0);
                    let green = channel(self, 8.0);
                    let blue = channel(self, 4.0);
                    return Ok(self.push_call("customColor", vec![red, green, blue, alpha]));
                }
                if name == "timeToString" {
                    let [time] = args.as_slice() else {
                        return Err(self.unsupported("timeToString requires one argument", span));
                    };
                    // The reference slices padded numbers directly with `True`
                    // as the start index; neither the Number in the `stringSlice`
                    // string position nor the Boolean in its start position is
                    // canonical, so the padding goes through `customString` and
                    // the start index stays `1` (approved exception,
                    // docs/architecture/language-core.md).
                    let time = self.lower_value(time)?;
                    let three_thousand_six_hundred = self.push_number(3600.0);
                    let sixty = self.push_number(60.0);
                    let hour_value =
                        self.push_call("divide", vec![time, three_thousand_six_hundred]);
                    let down = self.push_value(Value::Enum {
                        value_type: "Rounding".to_string(),
                        value: "DOWN".to_string(),
                    });
                    let hour = self.push_call("roundToInteger", vec![hour_value, down]);
                    let minute_remainder =
                        self.push_call("modulo", vec![time, three_thousand_six_hundred]);
                    let minute = self.push_call("divide", vec![minute_remainder, sixty]);
                    let second = self.push_call("modulo", vec![time, sixty]);
                    let hundred = self.push_number(100.0);
                    let start = self.push_number(1.0);
                    let two = self.push_number(2.0);
                    let all_digits = self.push_number(9999.0);
                    let minute_with_padding = self.push_call("add", vec![minute, hundred]);
                    let second_with_padding = self.push_call("add", vec![second, hundred]);
                    // The reference emits its `substring` calls unevaluated, so
                    // the canonical wrapper and slices stay unfolded even when
                    // `time` is a constant.
                    let optimization = self.optimization_mark.take();
                    let padding_template = self.push_value(Value::String("{0}".to_string()));
                    let minute_with_padding =
                        self.push_call("customString", vec![padding_template, minute_with_padding]);
                    let minute_text =
                        self.push_call("stringSlice", vec![minute_with_padding, start, two]);
                    let second_with_padding =
                        self.push_call("customString", vec![padding_template, second_with_padding]);
                    let second_text =
                        self.push_call("stringSlice", vec![second_with_padding, start, all_digits]);
                    self.optimization_mark = optimization;
                    let template = self.push_value(Value::String("{0}:{1}:{2}".to_string()));
                    return Ok(self.push_call(
                        "customString",
                        vec![template, hour, minute_text, second_text],
                    ));
                }
                if name == "compressed" {
                    return self.lower_compressed(args, span);
                }
                if name == "compress" {
                    return self.lower_compress(args, span);
                }
                if name == "getSign" {
                    let [number] = args.as_slice() else {
                        return Err(self.unsupported("getSign requires one argument", span));
                    };
                    let number = self.lower_value(number)?;
                    let infinity = self.push_number(999_999_999_999.0);
                    let scaled = self.push_call("multiply", vec![number, infinity]);
                    let scaled = self.push_call("multiply", vec![scaled, infinity]);
                    let scaled = self.push_call("divide", vec![scaled, infinity]);
                    let ten = self.push_number(10.0);
                    return Ok(self.push_call("divide", vec![scaled, ten]));
                }
                if name == "lerp" {
                    let [start, end, t] = args.as_slice() else {
                        return Err(self.unsupported("lerp requires three arguments", span));
                    };
                    let start = self.lower_value(start)?;
                    let end = self.lower_value(end)?;
                    let t = self.lower_value(t)?;
                    let one = self.push_number(1.0);
                    let weight = self.push_call("subtract", vec![one, t]);
                    let start_part = self.push_call("multiply", vec![start, weight]);
                    let end_part = self.push_call("multiply", vec![end, t]);
                    return Ok(self.push_call("add", vec![start_part, end_part]));
                }
                if name == "log" {
                    if !matches!(args.as_slice(), [_] | [_, _]) {
                        return Err(self.unsupported("log requires one or two arguments", span));
                    }
                    // The reference expands `log` to a power approximation,
                    // folding `Math.log` when the operand is a constant, so the
                    // expansion waits for the optimized arguments at
                    // materialization.
                    let args = args
                        .iter()
                        .map(|arg| self.lower_value(arg))
                        .collect::<Result<_, _>>()?;
                    return Ok(self.push_call("log", args));
                }
                if name == "getCurrentMap" && args.is_empty() && !self.used_maps.is_empty() {
                    return Ok(self.lower_bugged_current_map());
                }
                if matches!(name.as_str(), "attacker" | "victim") && args.is_empty() {
                    return Ok(self.push_call(name, Vec::new()));
                }
                if name == "localPlayer" && args.is_empty() {
                    return Ok(self.push_call(name, Vec::new()));
                }
                if name == "ruleCondition" {
                    if !args.is_empty() {
                        return Err(
                            self.unsupported("ruleCondition does not accept arguments", span)
                        );
                    }
                    let conditions = self.current_rule_conditions.clone().ok_or_else(|| {
                        self.unsupported("ruleCondition is only valid inside a rule", span)
                    })?;
                    return Ok(self
                        .combine_conditions(conditions)
                        .unwrap_or_else(|| self.push_value(Value::Bool(true))));
                }
                if name == "vect" && args.len() == 3 {
                    let x = self.lower_value(&args[0])?;
                    let y = self.lower_value(&args[1])?;
                    let z = self.lower_value(&args[2])?;
                    if let Some(member) = self.canonical_vector_member(x, y, z) {
                        Value::Enum {
                            value_type: "Vector".to_string(),
                            value: member.to_string(),
                        }
                    } else {
                        Value::Vector { x, y, z }
                    }
                } else if matches!(
                    name.as_str(),
                    "createWorkshopSettingBool"
                        | "createWorkshopSettingEnum"
                        | "createWorkshopSettingInt"
                        | "createWorkshopSettingFloat"
                        | "createWorkshopSettingHero"
                ) {
                    let mut lowered = self.lower_values(args)?;
                    // The sort order is the last parameter; OverPy writes 0 when omitted.
                    let (canonical, arity_without_sort_order) = workshop_setting_call(name);
                    if lowered.len() == arity_without_sort_order {
                        lowered.push(self.push_number(0.0));
                    }
                    Value::Call {
                        name: canonical.to_string(),
                        args: self.value_args(&lowered),
                    }
                } else if matches!(name.as_str(), "all" | "any") {
                    let call_name = if name == "all" {
                        "isTrueForAll"
                    } else {
                        "isTrueForAny"
                    };
                    let [array] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{name} requires exactly one array argument"),
                            span,
                        ));
                    };
                    let (array, condition) = match array {
                        Expr::Comprehension {
                            element,
                            variable,
                            index,
                            iterable,
                            ..
                        } => {
                            if index.is_some() {
                                return Err(self.unsupported(
                                    format!("{name} does not support an index binder"),
                                    span,
                                ));
                            }
                            let iterable = self.lower_value(iterable)?;
                            self.array_bindings.push(ArrayBinding {
                                element: variable.clone(),
                                index: None,
                            });
                            let condition = self.lower_value(element);
                            self.array_bindings.pop();
                            (iterable, condition?)
                        }
                        array => (
                            self.lower_value(array)?,
                            self.push_call("currentArrayElement", Vec::new()),
                        ),
                    };
                    Value::Call {
                        name: call_name.to_string(),
                        args: self.value_args(&[array, condition]),
                    }
                } else if matches!(name.as_str(), "ceil" | "floor" | "round") {
                    let [value] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{name} requires exactly one numeric argument"),
                            span,
                        ));
                    };
                    let rounding = match name.as_str() {
                        "ceil" => "UP",
                        "floor" => "DOWN",
                        "round" => "NEAREST",
                        _ => unreachable!(),
                    };
                    let rounding = self.push_value(Value::Enum {
                        value_type: "Rounding".to_string(),
                        value: rounding.to_string(),
                    });
                    let value = self.lower_value(value)?;
                    Value::Call {
                        name: "roundToInteger".to_string(),
                        args: self.value_args(&[value, rounding]),
                    }
                } else if name == "sorted" {
                    let (array_expr, key) = match args.as_slice() {
                        [array] => (array, self.push_call("currentArrayElement", Vec::new())),
                        [
                            array,
                            Expr::Lambda {
                                params, body, span, ..
                            },
                        ] => {
                            let key = self.lower_array_callback(params, body, *span)?;
                            (array, key)
                        }
                        _ => {
                            return Err(self.unsupported(
                                "sorted requires an array and an optional lambda key",
                                span,
                            ));
                        }
                    };
                    // The reference folds `sorted` on an array literal whose
                    // key is `-Current Array Index` into the reversed literal.
                    if self.is_reversed_index_key(key)
                        && let Some(elements) = self.lower_reversed_literal_array(array_expr)?
                    {
                        return Ok(elements);
                    }
                    let array = self.lower_value(array_expr)?;
                    Value::Call {
                        name: "sortedArray".to_string(),
                        args: self.value_args(&[array, key]),
                    }
                } else {
                    let function = self
                        .compiler
                        .manifest
                        .resolve_function(name)
                        .ok_or_else(|| self.unsupported(format!("unknown value '{name}'"), span))?;
                    if !matches!(function.kind, FunctionKind::Value) {
                        return Err(
                            self.unsupported(format!("'{name}' is not a generic OPY value"), span)
                        );
                    }
                    let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
                        self.unsupported(
                            format!(
                                "value '{}' requires a special lowering not in #46",
                                function.id
                            ),
                            span,
                        )
                    })?;
                    if function.id == "getAllPlayers" {
                        return Ok(self.lower_all_players());
                    }
                    let lowered_args = self.lower_values(args)?;
                    Value::Call {
                        name: catalog_id.clone(),
                        args: self.value_args(&lowered_args),
                    }
                }
            }
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => {
                if name == "getOppositeTeam" {
                    if !args.is_empty() {
                        return Err(self.unsupported("getOppositeTeam requires no arguments", span));
                    }
                    let receiver = self.lower_value(receiver)?;
                    let team = self.push_call("teamOf", vec![receiver]);
                    return Ok(self.push_call("oppositeTeamOf", vec![team]));
                }
                if name == "toArray" {
                    if !args.is_empty() {
                        return Err(self.unsupported("toArray requires no arguments", span));
                    }
                    let Expr::Type {
                        name: type_name, ..
                    } = receiver.as_ref()
                    else {
                        return Err(
                            self.unsupported("toArray requires an enum type receiver", span)
                        );
                    };
                    let domain_name = match type_name.as_str() {
                        "Clip" => "Clipping",
                        _ => type_name.as_str(),
                    };
                    let Some(domain) = self.compiler.catalog.enum_domain(domain_name) else {
                        return Err(
                            self.unsupported(format!("unknown enum type '{type_name}'"), span)
                        );
                    };
                    let values = domain
                        .members
                        .iter()
                        .map(|member| {
                            self.push_value(Value::Enum {
                                value_type: domain_name.to_string(),
                                value: member.member.clone(),
                            })
                        })
                        .collect();
                    return Ok(self.push_call("array", values));
                }
                if matches!(name.as_str(), "all" | "any") {
                    let receiver = self.lower_value(receiver)?;
                    let condition = match args.as_slice() {
                        [] => self.push_call("currentArrayElement", Vec::new()),
                        [
                            Expr::Lambda {
                                params, body, span, ..
                            },
                        ] => self.lower_array_callback(params, body, *span)?,
                        _ => {
                            return Err(self.unsupported(
                                format!("{name} requires zero or one lambda argument"),
                                span,
                            ));
                        }
                    };
                    let args = self.value_args(&[receiver, condition]);
                    return Ok(self.push_value(Value::Call {
                        name: if name == "all" {
                            "isTrueForAll"
                        } else {
                            "isTrueForAny"
                        }
                        .to_string(),
                        args,
                    }));
                }
                let function = self.compiler.manifest.resolve_member(name).ok_or_else(|| {
                    self.unsupported(format!("unknown member value '{name}'"), span)
                })?;
                if !matches!(function.kind, FunctionKind::MemberValue) {
                    return Err(self.unsupported(format!("'{name}' is not a member value"), span));
                }
                if function.id == "unique" {
                    if !args.is_empty() {
                        return Err(self.unsupported("unique requires no arguments", span));
                    }
                    let receiver = self.lower_value(receiver)?;
                    let current_element = self.push_call("currentArrayElement", Vec::new());
                    let first_index =
                        self.push_call("indexOfArrayValue", vec![receiver, current_element]);
                    let current_index = self.push_call("currentArrayIndex", Vec::new());
                    let condition = self.push_call("==", vec![first_index, current_index]);
                    return Ok(self.push_call("filteredArray", vec![receiver, condition]));
                }
                if function.id == "reverse" {
                    if !args.is_empty() {
                        return Err(self.unsupported("reverse requires no arguments", span));
                    }
                    // `x.reverse()` expands to `sorted` on the `-Current Array
                    // Index` key, which the reference folds into the reversed
                    // literal when `x` is an array literal.
                    if let Some(elements) = self.lower_reversed_literal_array(receiver)? {
                        return Ok(elements);
                    }
                    let receiver = self.lower_value(receiver)?;
                    let index = self.push_call("currentArrayIndex", Vec::new());
                    let key = self.push_call("-", vec![index]);
                    return Ok(self.push_call("sortedArray", vec![receiver, key]));
                }
                if function.id == "getEffectiveHero" {
                    if !args.is_empty() {
                        return Err(
                            self.unsupported("getEffectiveHero requires no arguments", span)
                        );
                    }
                    let receiver = self.lower_value(receiver)?;
                    let duplicated = self.push_call("getHeroOfDuplication", vec![receiver]);
                    let hero = self.push_call("getHero", vec![receiver]);
                    let null = self.push_value(Value::Null);
                    let condition = self.push_call("==", vec![duplicated, null]);
                    return Ok(self.push_call("ifThenElse", vec![condition, hero, duplicated]));
                }
                if function.id == "getRealPlayersInViewAngle" {
                    let [team, view_angle] = args.as_slice() else {
                        return Err(self.unsupported(
                            "getRealPlayersInViewAngle requires team and view angle",
                            span,
                        ));
                    };
                    let receiver = self.lower_value(receiver)?;
                    let team = self.lower_value(team)?;
                    let view_angle = self.lower_value(view_angle)?;
                    let players =
                        self.push_call("getPlayersInViewAngle", vec![receiver, team, view_angle]);
                    let current = self.push_call("currentArrayElement", Vec::new());
                    let alive = self.push_call("isAlive", vec![current]);
                    let spawned = self.push_call("hasSpawned", vec![current]);
                    let condition = self.push_call("and", vec![alive, spawned]);
                    return Ok(self.push_call("filteredArray", vec![players, condition]));
                }
                if matches!(
                    function.id.as_str(),
                    "getRealPlayerClosestToReticle" | "getRealPlayersClosestToReticle"
                ) {
                    let [team] = args.as_slice() else {
                        return Err(self
                            .unsupported("getRealPlayersClosestToReticle requires a team", span));
                    };
                    let receiver = self.lower_value(receiver)?;
                    let team = self.lower_value(team)?;
                    let players = self.push_call("getLivingPlayers", vec![team]);
                    let current = self.push_call("currentArrayElement", Vec::new());
                    let spawned = self.push_call("hasSpawned", vec![current]);
                    let not_self = self.push_call("!=", vec![current, receiver]);
                    let condition = self.push_call("and", vec![spawned, not_self]);
                    let players = self.push_call("filteredArray", vec![players, condition]);
                    let facing = self.push_call("getFacingDirection", vec![receiver]);
                    let eye_position = self.push_call("getEyePosition", vec![receiver]);
                    let direction = self.push_call("subtract", vec![current, eye_position]);
                    let angle = self.push_call("angleBetweenVectors", vec![facing, direction]);
                    let sorted = self.push_call("sortedArray", vec![players, angle]);
                    return if function.id == "getRealPlayerClosestToReticle" {
                        Ok(self.push_call("firstOf", vec![sorted]))
                    } else {
                        Ok(sorted)
                    };
                }
                if function.id == "map" {
                    let [
                        Expr::Lambda {
                            params, body, span, ..
                        },
                    ] = args.as_slice()
                    else {
                        return Err(self.unsupported("map requires one lambda argument", span));
                    };
                    let mapped = self.lower_array_callback(params, body, *span)?;
                    let receiver = self.lower_value(receiver)?;
                    return Ok(self.push_call("mappedArray", vec![receiver, mapped]));
                }
                if matches!(
                    function.id.as_str(),
                    "getHitPosition" | "getPlayerHit" | "getNormal"
                ) {
                    let member_name = function.id.as_str();
                    let Expr::Call {
                        name: receiver_name,
                        args: receiver_args,
                        ..
                    } = receiver.as_ref()
                    else {
                        return Err(self.unsupported(
                            format!("{member_name} requires a raycast receiver"),
                            span,
                        ));
                    };
                    if receiver_name != "raycast" || !args.is_empty() {
                        return Err(self.unsupported(
                            format!("{member_name} requires raycast(...) with no member arguments"),
                            span,
                        ));
                    }
                    let catalog_id = function.catalog_id.clone().ok_or_else(|| {
                        self.unsupported(
                            format!("{member_name} has no canonical catalog identity"),
                            span,
                        )
                    })?;
                    let lowered_args = self.lower_values(receiver_args)?;
                    return Ok(self.push_value(Value::Call {
                        name: catalog_id,
                        args: self.value_args(&lowered_args),
                    }));
                }
                if function.id == "filter" {
                    let [
                        Expr::Lambda {
                            params, body, span, ..
                        },
                    ] = args.as_slice()
                    else {
                        return Err(self.unsupported("filter requires one lambda argument", span));
                    };
                    let condition = self.lower_array_callback(params, body, *span)?;
                    let receiver = self.lower_value(receiver)?;
                    Value::Call {
                        name: "filteredArray".to_string(),
                        args: self.value_args(&[receiver, condition]),
                    }
                } else if matches!(function.id.as_str(), "concat" | "exclude") {
                    let [value] = args.as_slice() else {
                        return Err(self.unsupported(
                            format!("{} requires exactly one argument", function.id),
                            span,
                        ));
                    };
                    let receiver = self.lower_value(receiver)?;
                    let value = self.lower_value(value)?;
                    Value::Call {
                        name: if function.id == "concat" {
                            "appendToArray"
                        } else {
                            "removeFromArray"
                        }
                        .to_string(),
                        args: self.value_args(&[receiver, value]),
                    }
                } else {
                    let catalog_id = function.catalog_id.as_ref().ok_or_else(|| {
                        self.unsupported(
                            format!(
                                "member value '{}' has no canonical catalog identity",
                                function.id
                            ),
                            span,
                        )
                    })?;
                    let mut lowered = Vec::with_capacity(args.len() + 1);
                    lowered.push(self.lower_value(receiver)?);
                    lowered.extend(self.lower_values(args)?);
                    Value::Call {
                        name: catalog_id.clone(),
                        args: self.value_args(&lowered),
                    }
                }
            }
            Expr::Member {
                receiver, member, ..
            } => {
                let receiver = self.lower_value(receiver)?;
                if let Some(name) = match member.as_str() {
                    "x" => Some("__xComponentOf__"),
                    "y" => Some("__yComponentOf__"),
                    "z" => Some("__zComponentOf__"),
                    _ => None,
                } {
                    Value::Call {
                        name: name.to_string(),
                        args: self.value_args(&[receiver]),
                    }
                } else {
                    let member = self.push_value(Value::String(member.clone()));
                    Value::Call {
                        name: "memberAccess".to_string(),
                        args: self.value_args(&[receiver, member]),
                    }
                }
            }
            Expr::Comprehension {
                element,
                variable,
                index,
                iterable,
                condition,
                span: comprehension_span,
                ..
            } => {
                if condition.is_some() && index.is_some() {
                    return Err(self.unsupported(
                        "comprehensions with both a filter and an index binder are not currently representable in canonical WIR",
                        *comprehension_span,
                    ));
                }
                let iterable = self.lower_value(iterable)?;
                let iterable = if self.value_is_known_player(iterable) {
                    self.push_call("array", vec![iterable])
                } else {
                    iterable
                };
                let binding = ArrayBinding {
                    element: variable.clone(),
                    index: index.clone(),
                };
                self.array_bindings.push(binding);
                let predicate = condition
                    .as_deref()
                    .map(|condition| self.lower_value(condition));
                let element = self.lower_value(element);
                self.array_bindings.pop();
                let element = element?;
                let iterable = if let Some(predicate) = predicate {
                    let predicate = predicate?;
                    self.push_call("filteredArray", vec![iterable, predicate])
                } else {
                    iterable
                };
                Value::Call {
                    name: "mappedArray".to_string(),
                    args: self.value_args(&[iterable, element]),
                }
            }
            Expr::Lambda { span, .. } => {
                return Err(self.unsupported(
                    "lambda expressions are only representable as supported array operation arguments",
                    *span,
                ));
            }
            Expr::StringModifier {
                modifier,
                value,
                span,
            } => {
                let value = match modifier.as_str() {
                    "b" => big_letters(value),
                    "c" => case_sensitive(value),
                    "w" => fullwidth(value),
                    _ => {
                        return Err(self.unsupported(
                            format!(
                                "string modifier '{modifier}' is not currently representable in canonical WIR"
                            ),
                            *span,
                        ));
                    }
                };
                return Ok(self.lower_custom_string(value));
            }
            _ => {
                return Err(self.unsupported(
                    format!(
                        "expression '{}' is not currently representable in canonical WIR",
                        expr.kind_name()
                    ),
                    span,
                ));
            }
        };
        let value_id = self.push_value(value);
        let Some(Value::Call { name, args }) = self.values.get(value_id) else {
            return Ok(value_id);
        };
        let name = name.clone();
        let args = args.clone();
        let mut args = self.normalize_contextual_arguments(&name, args);
        self.apply_replacements_to_values(&name, &mut args, span);
        if let Some(Value::Call {
            args: target_args, ..
        }) = self.values.get_mut(value_id)
        {
            *target_args = args;
        }
        Ok(value_id)
    }

    /// `getCurrentMap() == Map.X`: the maps whose value comparison the
    /// Workshop gets wrong are compared as text instead.
    fn lower_current_map_equality(&mut self, left: &Expr, right: &Expr) -> Option<ValueId> {
        let is_current_map = |expr: &Expr| matches!(expr, Expr::Call { name, args, .. } if name == "getCurrentMap" && args.is_empty());
        let map_of = |expr: &Expr| match expr {
            Expr::Enum {
                value_type, value, ..
            } if value_type == "Map" => Some(value.clone()),
            _ => None,
        };
        let map = match (map_of(left), map_of(right)) {
            (Some(map), None) if is_current_map(right) => map,
            (None, Some(map)) if is_current_map(left) => map,
            _ => return None,
        };
        let current = self.push_call("currentMap", Vec::new());
        let map_value = self.push_value(Value::Enum {
            value_type: "Map".to_string(),
            value: map.clone(),
        });
        if !TEXT_COMPARED_MAPS.contains(&map.as_str()) {
            return Some(self.push_call("==", vec![current, map_value]));
        }
        let format = self.push_value(Value::String("{0}".to_string()));
        let current_text = self.push_call("customString", vec![format, current]);
        let format = self.push_value(Value::String("{0}".to_string()));
        let map_text = self.push_call("customString", vec![format, map_value]);
        Some(self.push_call("==", vec![current_text, map_text]))
    }

    /// A bare `getCurrentMap()` selects the used map from the bugged ones by
    /// its text, since comparing the map values themselves fails for them.
    fn lower_bugged_current_map(&mut self) -> ValueId {
        let mut maps: Vec<ValueId> = self
            .used_maps
            .clone()
            .into_iter()
            .map(|map| {
                self.push_value(Value::Enum {
                    value_type: "Map".to_string(),
                    value: map.to_string(),
                })
            })
            .collect();
        maps.push(self.push_call("currentMap", Vec::new()));
        let candidates = self.push_call("array", maps);
        let current = self.push_call("currentMap", Vec::new());
        let format = self.push_value(Value::String("{0}".to_string()));
        let current_text = self.push_call("customString", vec![format, current]);
        let element = self.push_call("currentArrayElement", Vec::new());
        let empty = self.push_call("emptyArray", Vec::new());
        let element_text = self.push_call("stringSplit", vec![element, empty]);
        let matches = self.push_call("==", vec![current_text, element_text]);
        let filtered = self.push_call("filteredArray", vec![candidates, matches]);
        self.push_call("firstOf", vec![filtered])
    }

    pub(super) fn apply_replacements_to_values(
        &mut self,
        call_id: &str,
        args: &mut [ValueId],
        span: Option<HirSpan>,
    ) {
        for value in args {
            *value = self.apply_replacement(*value, call_id, span);
        }
    }

    fn apply_replacement(
        &mut self,
        value_id: ValueId,
        call_id: &str,
        span: Option<HirSpan>,
    ) -> ValueId {
        let optimization = self.optimization_state_at(span.as_ref());
        if !optimization.enabled
            || !optimization.for_size
            || matches!(
                call_id,
                "workshopSettingToggle"
                    | "workshopSettingCombo"
                    | "workshopSettingInteger"
                    | "workshopSettingFloat"
            )
        {
            return value_id;
        }
        let replacement = |name: &str, hir: &hir::Program| {
            hir.preprocessing
                .replacements
                .iter()
                .find(|value| value.value == name)
                .is_some()
        };
        match self.value(value_id).clone() {
            Value::Number(0.0) => {
                let name = [
                    "getCapturePercentage",
                    "getPayloadProgressPercentage",
                    "isMatchComplete",
                ]
                .into_iter()
                .find(|name| replacement(name, self.hir));
                name.map_or(value_id, |name| self.push_call(name, Vec::new()))
            }
            Value::Number(1.0) => {
                if replacement("getMatchRound", self.hir) {
                    self.push_call("getMatchRound", Vec::new())
                } else {
                    value_id
                }
            }
            Value::Enum { value_type, value } if value_type == "Team" && value == "TEAM_1" => {
                if replacement("getControlScoringTeam", self.hir) {
                    self.push_call("getControlScoringTeam", Vec::new())
                } else {
                    value_id
                }
            }
            Value::String(value) if value.is_empty() => {
                if replacement("emptyArray", self.hir) {
                    self.push_call("emptyArray", Vec::new())
                } else if replacement("variable", self.hir) {
                    self.push_value(Value::GlobalVariable(EMPTY_STRING_NAME.to_string()))
                } else {
                    value_id
                }
            }
            Value::Call { name, args }
                if name == "customString"
                    && args.len() == 1
                    && self.value_is_empty_string(args[0]) =>
            {
                if replacement("emptyArray", self.hir) {
                    self.push_call("emptyArray", Vec::new())
                } else if replacement("variable", self.hir) {
                    self.push_value(Value::GlobalVariable(EMPTY_STRING_NAME.to_string()))
                } else {
                    value_id
                }
            }
            _ => value_id,
        }
    }

    fn lower_compressed(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        self.lower_compressed_mode(args, span, true)
    }

    fn lower_decompression(
        &mut self,
        text: &Expr,
        is_vector: bool,
    ) -> Result<ValueId, IntegrationError> {
        let text = self.lower_value(text)?;
        let (decoded, alphabet, variable_alphabet) = self.lower_compression_source(text);
        let (width, min_decimal_place, offset) = if is_vector {
            (3, -2.0, 5000.0)
        } else {
            (4, -3.0, 50000.0)
        };
        let component = |this: &mut Self, component_offset| {
            let value = this.lower_compressed_component(
                alphabet,
                variable_alphabet,
                width,
                min_decimal_place,
                component_offset,
            );
            let offset = this.push_number(offset);
            this.push_call("subtract", vec![value, offset])
        };
        if is_vector {
            let x = component(self, 0);
            let y = component(self, width * 2);
            let z = component(self, width);
            let vector = self.push_call("vector", vec![x, y, z]);
            Ok(self.push_call("mappedArray", vec![decoded, vector]))
        } else {
            let number = component(self, 0);
            Ok(self.push_call("mappedArray", vec![decoded, number]))
        }
    }

    fn lower_compression_source(&mut self, text: ValueId) -> (ValueId, ValueId, bool) {
        let variable_alphabet = has_directive(self.hir, "useVariableForCompressionAlphabet");
        let null = self.push_value(Value::Null);
        let separator = self.push_call("firstOf", vec![null]);
        let split = self.push_call("stringSplit", vec![text, separator]);
        let alphabet = if variable_alphabet {
            let variable = *self
                .globals
                .get(COMPRESSION_ALPHABET_NAME)
                .expect("compression alphabet variable is created");
            self.push_value(Value::GlobalVariable(self.global_names[variable].clone()))
        } else {
            self.lower_custom_string(compression_alphabet())
        };
        let decoded = if variable_alphabet {
            split
        } else {
            let current = self.push_call("currentArrayElement", Vec::new());
            let alphabet = self.push_call("appendToArray", vec![current, alphabet]);
            self.push_call("mappedArray", vec![split, alphabet])
        };
        (decoded, alphabet, variable_alphabet)
    }

    fn lower_compressed_component(
        &mut self,
        alphabet: ValueId,
        variable_alphabet: bool,
        width: usize,
        min_decimal_place: f64,
        component_offset: usize,
    ) -> ValueId {
        let current = self.push_call("currentArrayElement", Vec::new());
        let mut terms = Vec::with_capacity(width);
        for index in 0..width {
            let position = self.push_number((index + component_offset) as f64);
            let character = self.push_call("charAt", vec![current, position]);
            let formula_alphabet = if variable_alphabet {
                alphabet
            } else {
                self.push_call("lastOf", vec![current])
            };
            let digit = self.push_call("strIndex", vec![formula_alphabet, character]);
            let power = 100_f64.powf(index as f64 + min_decimal_place / 2.0);
            let power = self.push_number(power);
            let weighted = self.push_call("multiply", vec![power, digit]);
            terms.push(weighted);
        }
        let mut value = terms
            .first()
            .copied()
            .unwrap_or_else(|| self.push_number(0.0));
        for term in terms.into_iter().skip(1) {
            value = self.push_call("add", vec![value, term]);
        }
        value
    }

    fn lower_compressed_mode(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
        decode: bool,
    ) -> Result<ValueId, IntegrationError> {
        let [Expr::Array { elements, .. }] = args else {
            return Err(self.unsupported(
                "compressed requires one literal array of numbers or vectors",
                span,
            ));
        };
        if elements.is_empty() {
            return Err(self.unsupported("cannot compress an empty array", span));
        }

        let Some(numbers) = elements
            .iter()
            .map(|element| match element {
                Expr::Null { .. } => Some(vec![0.0]),
                Expr::Number { value, .. } => Some(vec![*value]),
                Expr::Unary { op, operand, .. } if matches!(op.as_str(), "+" | "-") => {
                    hir::visit::literal_number(operand)
                        .map(|value| vec![if op == "-" { -value } else { value }])
                }
                Expr::Vector { x, y, z, .. } => Some(vec![
                    hir::visit::literal_number(x)?,
                    hir::visit::literal_number(y)?,
                    hir::visit::literal_number(z)?,
                ]),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Err(self.unsupported("compressed requires literal numbers or vectors", span));
        };
        let Some((is_vector, limit)) = crate::lower::compressed_component_mode(&numbers) else {
            return Err(self.unsupported("compressed cannot mix numbers and vectors", span));
        };
        let flattened = numbers.iter().flatten().copied().collect::<Vec<_>>();
        if flattened.iter().any(|value| value.abs() >= limit) {
            return Err(self.unsupported("compressed values exceed the supported magnitude", span));
        }

        let max_decimals = if is_vector { 2 } else { 3 };
        let compression_offset = if decode {
            flattened.iter().copied().fold(0.0_f64, f64::min).min(0.0)
        } else if is_vector {
            -5000.0
        } else {
            -50000.0
        };
        let adjusted = flattened
            .iter()
            .map(|value| value - compression_offset)
            .collect::<Vec<_>>();
        let mut strings = adjusted
            .iter()
            .map(|value| {
                format!("{value:.precision$}", precision = max_decimals)
                    .replace('.', "")
                    .chars()
                    .rev()
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let mut min_decimal_place = -(max_decimals as i32);
        if decode {
            while strings.iter().all(|value| value.starts_with('0')) {
                for value in &mut strings {
                    value.remove(0);
                }
                min_decimal_place += 1;
            }
        } else {
            min_decimal_place = if is_vector { -2 } else { -3 };
        }
        let max_decimal_place = if decode {
            min_decimal_place + strings.iter().map(String::len).max().unwrap_or_default() as i32
        } else if is_vector {
            4
        } else {
            5
        };
        for value in &mut strings {
            let trimmed = value.trim_end_matches('0');
            *value = if trimmed.is_empty() {
                "0".to_string()
            } else {
                trimmed.to_string()
            };
        }

        let alphabet = compression_alphabet_chars();
        let encode = |value: &str| -> Option<String> {
            let mut encoded = String::new();
            let chars = value.as_bytes();
            for pair in chars.chunks(2) {
                let number = if pair.len() == 1 {
                    u16::from(pair[0] - b'0')
                } else {
                    u16::from(pair[1] - b'0') * 10 + u16::from(pair[0] - b'0')
                };
                encoded.push(*alphabet.get(number as usize)?);
            }
            Some(encoded)
        };
        let compressed = if is_vector {
            let width = (((max_decimal_place - min_decimal_place + 1) / 2) * 2) as usize;
            strings
                .chunks(3)
                .map(|values| {
                    let mut grouped = String::new();
                    for index in [0, 2, 1] {
                        let mut value = values[index].clone();
                        if index != 1 {
                            value.push_str(&"0".repeat(width.saturating_sub(value.len())));
                        } else {
                            value = value.trim_end_matches('0').to_string();
                            if value.is_empty() {
                                value.push('0');
                            }
                        }
                        grouped.push_str(&value);
                    }
                    encode(&grouped)
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| self.unsupported("compressed value cannot be encoded", span))?
                .join("0")
        } else {
            strings
                .iter()
                .map(|value| encode(value))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| self.unsupported("compressed value cannot be encoded", span))?
                .join("0")
        };
        if !decode {
            return Ok(self.lower_custom_string(compressed));
        }
        let compressed_string = self.lower_custom_string(compressed);
        let (decoded, alphabet, variable_alphabet) =
            self.lower_compression_source(compressed_string);
        let width = ((max_decimal_place - min_decimal_place + 1) / 2) as usize;
        let component = |this: &mut Self, component_offset| {
            this.lower_compressed_component(
                alphabet,
                variable_alphabet,
                width,
                f64::from(min_decimal_place),
                component_offset,
            )
        };
        let value = if is_vector {
            let x = component(self, 0);
            let y = component(self, width * 2);
            let z = component(self, width);
            let vector = self.push_call("vector", vec![x, y, z]);
            let value = if compression_offset == 0.0 {
                vector
            } else {
                let offset = self.push_number(-compression_offset);
                let offset = self.push_call("vector", vec![offset, offset, offset]);
                self.push_call("subtract", vec![vector, offset])
            };
            self.push_call("mappedArray", vec![decoded, value])
        } else {
            let number = component(self, 0);
            let number = if compression_offset == 0.0 {
                number
            } else {
                let offset = self.push_number(compression_offset);
                self.push_call("add", vec![number, offset])
            };
            self.push_call("mappedArray", vec![decoded, number])
        };
        Ok(value)
    }

    fn lower_compress(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        self.lower_compressed_mode(args, span, false)
    }

    fn strict_optimization_active(&self, expr: &Expr) -> bool {
        self.optimization_state_at(expr.span()).strict
    }

    pub(super) fn optimization_state_at(&self, span: Option<&HirSpan>) -> OptimizationState {
        if let Some(state) = &self.optimization_override {
            return state.clone();
        }
        let Some(span) = span else {
            return self.hir.preprocessing.optimization.clone();
        };
        let mut active = None;
        for directive in &self.hir.preprocessing.directives {
            let Some(directive_span) = directive.span else {
                continue;
            };
            if directive_span.file != span.file {
                continue;
            }
            if directive_span.start.line < span.start.line
                || (directive_span.start.line == span.start.line
                    && directive_span.start.col <= span.start.col)
            {
                active = Some(directive.state.optimization.clone());
            } else {
                break;
            }
        }
        active
            .or_else(|| {
                self.hir
                    .preprocessing
                    .source_file_initial_optimization
                    .get(&span.file)
                    .cloned()
            })
            .unwrap_or_else(|| self.hir.preprocessing.optimization.clone())
    }

    /// The array literal behind `expr`, following `const`/`macro`
    /// substitution like the reference's `__array__` check, lowered in
    /// reverse. Returns `None` when `expr` is not an array literal.
    fn lower_reversed_literal_array(
        &mut self,
        expr: &Expr,
    ) -> Result<Option<ValueId>, IntegrationError> {
        let mut definition = expr;
        let mut substituted = false;
        for _ in 0..=self.constants.len() {
            let Expr::Constant { name, .. } = definition else {
                break;
            };
            let Some(next) = self.constants.get(name).copied() else {
                return Ok(None);
            };
            substituted = true;
            definition = next;
        }
        let Expr::Array { elements, .. } = definition else {
            return Ok(None);
        };
        // As in `Expr::Constant`, a substituted expression optimizes under
        // the use-site state.
        let previous = substituted.then(|| {
            let state = self.optimization_state_at(expr.span());
            self.optimization_override.replace(state)
        });
        let elements = elements
            .iter()
            .rev()
            .map(|element| self.lower_value(element))
            .collect::<Result<Vec<_>, _>>();
        if let Some(previous) = previous {
            self.optimization_override = previous;
        }
        Ok(Some(self.lower_array(elements?)))
    }

    /// Whether `key` is the reference's `-Current Array Index` sort key —
    /// `multiply` of `-1` and the index in either order, or unary minus on
    /// the index — under which `sorted` folds an array literal into its
    /// reverse.
    fn is_reversed_index_key(&self, key: ValueId) -> bool {
        let is_index = |id: ValueId| {
            matches!(
                self.value(id),
                Value::Call { name, args } if name == "currentArrayIndex" && args.is_empty()
            )
        };
        let is_negative_one = |id: ValueId| matches!(self.value(id), Value::Number(-1.0));
        let Value::Call { name, args } = self.value(key) else {
            return false;
        };
        match (name.as_str(), args.as_slice()) {
            ("-", [operand]) => is_index(*operand),
            ("multiply", [left, right]) => {
                (is_negative_one(*left) && is_index(*right))
                    || (is_index(*left) && is_negative_one(*right))
            }
            _ => false,
        }
    }

    fn lower_array_callback(
        &mut self,
        params: &[String],
        body: &Expr,
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        if !(1..=2).contains(&params.len()) {
            return Err(self.unsupported(
                "array callbacks require one element parameter and at most one index parameter",
                span,
            ));
        }
        if params.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(
                self.unsupported("array callback parameters must have distinct names", span)
            );
        }
        self.array_bindings.push(ArrayBinding {
            element: params[0].clone(),
            index: params.get(1).cloned(),
        });
        let result = self.lower_value(body);
        self.array_bindings.pop();
        result
    }

    fn lower_workshop_setting(
        &mut self,
        args: &[Expr],
        span: Option<HirSpan>,
    ) -> Result<ValueId, IntegrationError> {
        let [
            Expr::Type {
                name: setting_type,
                args: type_args,
                span: type_span,
            },
            category,
            setting_name,
            default,
            sort_order,
        ] = args
        else {
            return Err(self.unsupported(
                "createWorkshopSetting requires a type and four value arguments",
                span,
            ));
        };

        let catalog_name = match (setting_type.as_str(), type_args.as_slice()) {
            ("bool", []) => "createWorkshopSettingBool",
            ("int", [_, _]) => "createWorkshopSettingInt",
            ("float", [_, _]) => "createWorkshopSettingFloat",
            ("int", []) | ("float", []) => {
                return Err(self.unsupported(
                    format!("createWorkshopSetting type '{setting_type}' requires a numeric range"),
                    type_span.or(span),
                ));
            }
            _ => {
                return Err(self.unsupported(
                    format!("unsupported createWorkshopSetting type '{setting_type}'"),
                    type_span.or(span),
                ));
            }
        };

        // OverPy uses an ideographic space for an empty setting category so
        // the generated Workshop setting has a non-empty category value.
        let category = match category {
            Expr::String { value, .. } if value.is_empty() => {
                self.push_value(Value::String("\u{3000}".to_string()))
            }
            _ => self.lower_value(category)?,
        };
        let mut lowered = vec![
            category,
            self.lower_value(setting_name)?,
            self.lower_value(default)?,
        ];
        if let [minimum, maximum] = type_args.as_slice() {
            lowered.push(self.lower_value(minimum)?);
            lowered.push(self.lower_value(maximum)?);
        }
        lowered.push(self.lower_value(sort_order)?);
        Ok(self.push_call(workshop_setting_call(catalog_name).0, lowered))
    }
}
