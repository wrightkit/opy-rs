use super::*;

impl Lowerer {
    pub(super) fn lower_expr(
        &mut self,
        expr: &Expr,
        macro_params: &[String],
        position: CallPosition,
    ) -> HirExpr {
        match expr {
            Expr::Number { value, text, span } => HirExpr::Number {
                value: *value,
                text: text.clone(),
                span: Some(span.into()),
            },
            Expr::String { value, span } => self.lower_string(value, *span),
            Expr::Bool { value, span } => HirExpr::Bool {
                value: *value,
                span: Some(span.into()),
            },
            Expr::Null { span } => HirExpr::Null {
                span: Some(span.into()),
            },
            Expr::Array { elements, span } => HirExpr::Array {
                elements: elements
                    .iter()
                    .map(|element| self.lower_expr(element, macro_params, CallPosition::Value))
                    .collect(),
                span: Some(span.into()),
            },
            Expr::Dict { entries, span } => {
                if !self.allow_dict_literal {
                    self.error_at(
                        "dict-access",
                        "dictionary literals must be accessed by a key".to_string(),
                        *span,
                    );
                    return HirExpr::Null { span: None };
                }
                HirExpr::Dict {
                    entries: entries
                        .iter()
                        .map(|entry| HirDictEntry {
                            key: Box::new(self.lower_expr(
                                &entry.key,
                                macro_params,
                                CallPosition::Value,
                            )),
                            value: Box::new(self.lower_expr(
                                &entry.value,
                                macro_params,
                                CallPosition::Value,
                            )),
                            span: Some(entry.span.into()),
                        })
                        .collect(),
                    span: Some(span.into()),
                }
            }
            Expr::Comprehension {
                element,
                variable,
                variable_span,
                index,
                iterable,
                condition,
                span,
            } => {
                let iterable = self.lower_expr(iterable, macro_params, CallPosition::Value);
                let previous = std::mem::take(&mut self.locals);
                self.locals.push(variable.clone());
                if let Some((index, _)) = index {
                    self.locals.push(index.clone());
                }
                let element = self.lower_expr(element, macro_params, CallPosition::Value);
                let condition = condition.as_ref().map(|condition| {
                    Box::new(self.lower_expr(condition, macro_params, CallPosition::Value))
                });
                self.locals = previous;
                HirExpr::Comprehension {
                    element: Box::new(element),
                    variable: variable.clone(),
                    variable_span: Some(variable_span.into()),
                    index: index.as_ref().map(|(name, _)| name.clone()),
                    index_span: index.as_ref().map(|(_, span)| (*span).into()),
                    iterable: Box::new(iterable),
                    condition,
                    span: Some(span.into()),
                }
            }
            Expr::Lambda {
                params,
                body,
                parenthesized,
                span,
            } => {
                // The reference's token-level lambda check never sees a
                // parenthesized lambda as a binder argument (issue #445).
                if position != CallPosition::LambdaArgument || *parenthesized {
                    self.error_at(
                        "lambda-context",
                        "lambda expressions are only valid as array operation arguments"
                            .to_string(),
                        *span,
                    );
                    return HirExpr::Null { span: None };
                }
                let previous = std::mem::take(&mut self.locals);
                self.locals = params.iter().map(|(name, _)| name.clone()).collect();
                let body = self.lower_expr(body, macro_params, CallPosition::Value);
                self.locals = previous;
                HirExpr::Lambda {
                    params: params.iter().map(|(name, _)| name.clone()).collect(),
                    param_spans: params
                        .iter()
                        .map(|(_, span)| Some((*span).into()))
                        .collect(),
                    body: Box::new(body),
                    span: Some(span.into()),
                }
            }
            Expr::StringModifier {
                modifier,
                value,
                format_text,
                interpolations,
                span,
            } => {
                if *modifier == 'f' {
                    if let Some(format_text) = format_text {
                        if !interpolations.is_empty() {
                            return HirExpr::Format {
                                text: format_text.clone(),
                                args: interpolations
                                    .iter()
                                    .map(|expr| {
                                        self.lower_expr(expr, macro_params, CallPosition::Value)
                                    })
                                    .collect(),
                                span: Some(span.into()),
                            };
                        }
                        return HirExpr::String {
                            value: format_text.clone(),
                            span: Some(span.into()),
                        };
                    }
                }
                HirExpr::StringModifier {
                    modifier: modifier.to_string(),
                    value: value.clone(),
                    span: Some(span.into()),
                }
            }
            Expr::Name { name, span } => self.lower_name(name, *span, macro_params),
            Expr::Type { name, args, span } => HirExpr::Type {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.lower_expr(arg, macro_params, CallPosition::Value))
                    .collect(),
                span: Some(span.into()),
            },
            Expr::Member {
                receiver,
                member,
                member_span,
                span,
            } => self.lower_member(receiver, member, *member_span, *span, macro_params),
            Expr::Index { array, index, span } => {
                let previous = self.allow_dict_literal;
                self.allow_dict_literal = true;
                let array = self.lower_expr(array, macro_params, CallPosition::Value);
                self.allow_dict_literal = previous;
                HirExpr::Index {
                    array: Box::new(array),
                    index: Box::new(self.lower_expr(index, macro_params, CallPosition::Value)),
                    span: Some(span.into()),
                }
            }
            Expr::Call {
                name,
                args,
                debug_source,
                span,
            } => self.lower_call(
                name,
                args,
                debug_source.as_deref(),
                *span,
                macro_params,
                position,
            ),
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                span,
            } => self.lower_receiver_call(receiver, name, args, *span, macro_params, position),
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => HirExpr::Binary {
                op: op.clone(),
                left: Box::new(self.lower_expr(left, macro_params, CallPosition::Value)),
                right: Box::new(self.lower_expr(right, macro_params, CallPosition::Value)),
                span: Some(span.into()),
            },
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                span,
            } => HirExpr::Conditional {
                then_value: Box::new(self.lower_expr(
                    then_value,
                    macro_params,
                    CallPosition::Value,
                )),
                condition: Box::new(self.lower_expr(condition, macro_params, CallPosition::Value)),
                else_value: Box::new(self.lower_expr(
                    else_value,
                    macro_params,
                    CallPosition::Value,
                )),
                span: Some((*span).into()),
            },
            Expr::Unary { op, operand, span } => HirExpr::Unary {
                op: op.clone(),
                operand: Box::new(self.lower_expr(operand, macro_params, CallPosition::Value)),
                span: Some(span.into()),
            },
        }
    }

    pub(super) fn lower_name(
        &mut self,
        name: &str,
        span: Span,
        macro_params: &[String],
    ) -> HirExpr {
        if macro_params.iter().any(|param| param == name) {
            return HirExpr::MacroParam {
                name: name.to_string(),
                span: Some(span.into()),
            };
        }
        if self.locals.iter().any(|local| local == name) {
            return HirExpr::Local {
                name: name.to_string(),
                span: Some(span.into()),
            };
        }
        if let Some(player) = context_player_expr(name, Some(span)) {
            return player;
        }
        match name {
            "RULE_CONDITION" | "ruleCondition" => HirExpr::Call {
                name: "ruleCondition".to_string(),
                args: Vec::new(),
                debug_source: None,
                span: Some(span.into()),
            },
            "eventAbility"
            | "eventDamage"
            | "eventDirection"
            | "eventHealing"
            | "eventWasCriticalHit"
            | "eventWasEnvironment"
            | "eventWasHealthPack" => HirExpr::Call {
                name: name.to_string(),
                args: Vec::new(),
                debug_source: None,
                span: Some(span.into()),
            },
            _ if self.global_visible(name) => HirExpr::GlobalVar {
                name: name.to_string(),
                span: Some(span.into()),
            },
            _ if self.player_visible(name) => HirExpr::PlayerVar {
                player: Box::new(HirExpr::EventPlayer { span: None }),
                name: name.to_string(),
                member_span: None,
                span: Some(span.into()),
            },
            _ if self.constant_visible(name) => HirExpr::Constant {
                name: name.to_string(),
                span: Some(span.into()),
            },
            _ if self.enum_visible(name) => {
                self.error_at(
                    "enum-type-without-member",
                    format!("enum type '{name}' must be used with a member (e.g. {name}.MEMBER)"),
                    span,
                );
                HirExpr::Null { span: None }
            }
            // OverPy default variable names (A–Z, AA–…, DX): implicit global
            // variables at fixed Workshop slots. The pinned reference accepts
            // these without a `globalvar` declaration anywhere a variable may
            // appear, including as a `for ... in range(...)` loop binder
            // (#114). Custom enums take precedence over default-var names,
            // matching the reference's identifier resolution order.
            _ if default_var_index(name).is_some() => HirExpr::GlobalVar {
                name: name.to_string(),
                span: Some(span.into()),
            },
            _ => {
                self.error_at(
                    "unknown-identifier",
                    format!("unknown identifier '{name}'"),
                    span,
                );
                HirExpr::Null { span: None }
            }
        }
    }

    pub(super) fn lower_member(
        &mut self,
        receiver: &Expr,
        member: &str,
        member_span: Span,
        span: Span,
        macro_params: &[String],
    ) -> HirExpr {
        if let Some(name) = self.qualified_macro_for_member(member) {
            return HirExpr::MacroCall {
                name,
                args: vec![self.lower_expr(receiver, macro_params, CallPosition::Value)],
                span: Some(span.into()),
            };
        }
        if let Expr::Name { name, .. } = receiver {
            // The reference rewrites `AsyncBehavior` to `StartRuleBehavior`
            // before the enum-domain lookup; keep the same effective name.
            let enum_name = match name.as_str() {
                "AsyncBehavior" => "StartRuleBehavior",
                other => other,
            };
            // Custom enum member: folds to its numeric constant. A member
            // missing from a custom enum falls through to a builtin domain
            // of the same name, matching the reference's per-member lookup.
            if self.enum_visible(enum_name) {
                let members = self
                    .enums
                    .get(enum_name)
                    .expect("enum span and members agree");
                if let Some(index) = members.iter().position(|candidate| candidate == member) {
                    return HirExpr::Number {
                        value: index as f64,
                        text: index.to_string(),
                        span: Some(span.into()),
                    };
                }
                self.error_at_closed_candidates(
                    "unknown-enum-member",
                    format!("enum '{name}' has no member '{member}'"),
                    span,
                    member,
                    &crate::matcher::bare_candidates(members.iter().cloned()),
                );
                return HirExpr::Null { span: None };
            }
            if name == "Texture" {
                if let Some(tag) = super::textures::tag(member) {
                    self.texture_used = true;
                    return HirExpr::Format {
                        text: tag.replacen('<', "{0}", 1),
                        args: vec![HirExpr::GlobalVar {
                            name: "__holygrail__".to_string(),
                            span: Some(span.into()),
                        }],
                        span: Some(span.into()),
                    };
                }
                self.error_at(
                    "unknown-texture-member",
                    format!("unknown texture member '{member}'"),
                    span,
                );
                return HirExpr::Null { span: None };
            }
            if name == "Math" {
                match member {
                    "FUCKTON_OF_SPACES" | "LOTS_OF_SPACES" => {
                        return HirExpr::String {
                            value: "\u{2003}".repeat(170),
                            span: Some(span.into()),
                        };
                    }
                    "FUCKTON_OF_NEWLINES" | "LOTS_OF_NEWLINES" => {
                        return HirExpr::String {
                            value: "\n".repeat(125),
                            span: Some(span.into()),
                        };
                    }
                    _ => {}
                }
                if let Some((value, text)) = match member {
                    "PI" => Some((std::f64::consts::PI, "3.141592653589793")),
                    "E" => Some((std::f64::consts::E, "2.718281828459045")),
                    "INFINITY" => Some((999_999_999_999.0, "999999999999")),
                    "EPSILON" => Some((1192093e-13, "0.0000001192093")),
                    "SPHERE_HORIZONTAL_RADIUS_MULT" => Some((0.984724, "0.984724")),
                    "SPHERE_VERTICAL_RADIUS_MULT" => Some((0.998959, "0.998959")),
                    "INNER_RING_RADIUS_MULT" => Some((0.9415, "0.9415")),
                    "OUTER_RING_RADIUS_MULT" => Some((0.94965, "0.94965")),
                    "RING_EXPLOSION_RADIUS_MULT" => Some((0.48, "0.48")),
                    _ => None,
                } {
                    return HirExpr::Number {
                        value,
                        text: text.to_string(),
                        span: Some(span.into()),
                    };
                }
            }
            if name == "Color" || name == "ColorLiteral" {
                // The four OverPy-only LIGHT_* constants are members of the
                // upstream `ColorLiteral` table. Through `Color.` the pinned
                // upstream lowers them to `rgb(r, g, b, 255)`; through
                // `ColorLiteral.` it accepts the same spellings but emits
                // the member's display-name lookup, which `onlyInOverpy`
                // members do not have — an empty argument slot the Workshop
                // grammar cannot parse. Emitting the canonical `rgb` form
                // here is a recorded exception
                // (docs/architecture/language-core.md, issue #466).
                if let Some((red, green, blue)) = crate::enums::extra_color_member(member) {
                    let number = |value: i32| HirExpr::Number {
                        value: f64::from(value),
                        text: value.to_string(),
                        span: Some(span.into()),
                    };
                    return HirExpr::Call {
                        name: "rgb".to_string(),
                        // The reference binds the declared alpha default
                        // (255) for these OverPy-only constants.
                        args: vec![number(red), number(green), number(blue), number(255)],
                        debug_source: None,
                        span: Some(span.into()),
                    };
                }
            }
            // Builtin Workshop enum: only the domain names and member
            // spellings the pinned upstream compiler exposes are accepted —
            // catalog member ids are not source spellings. `crate::enums`
            // holds the OPY spelling table both this resolution and the
            // lookup derive from; the `*Literal` receivers share their base
            // domain's member surface (issue #466).
            let catalog_domain = crate::enums::catalog_domain(name);
            if (!policy::is_contextual_domain(name) && self.manifest.domain_identity(name))
                || self.catalog.enum_domain(name).is_some()
                || self.catalog.enum_domain(catalog_domain).is_some()
                || crate::enums::literal_domain(name).is_some()
                || (name == "Clip" && self.manifest.domain_identity(catalog_domain))
            {
                match crate::enums::canonical_member(name, member, &self.catalog) {
                    Some((domain, canonical_member)) => {
                        return HirExpr::Enum {
                            value_type: domain,
                            value: canonical_member,
                            span: Some(span.into()),
                        };
                    }
                    None => {
                        match crate::enums::member_rejection(name, member) {
                            crate::enums::EnumMemberError::Misspelled(spelling) => {
                                self.error_at(
                                    "unknown-enum-member",
                                    format!(
                                        "enum '{name}' has no member '{member}'; the OverPy \
                                         spelling is '{name}.{spelling}'"
                                    ),
                                    span,
                                );
                            }
                            crate::enums::EnumMemberError::Unspellable => {
                                self.error_at(
                                    "unknown-enum-member",
                                    format!(
                                        "enum '{name}' has no member '{member}'; the canonical \
                                         member has no OverPy spelling"
                                    ),
                                    span,
                                );
                            }
                            crate::enums::EnumMemberError::Unknown => {
                                self.error_at_closed_candidates(
                                    "unknown-enum-member",
                                    format!("enum '{name}' has no member '{member}'"),
                                    span,
                                    member,
                                    &crate::matcher::enum_member_candidates(&self.catalog, name),
                                );
                            }
                        }
                        return HirExpr::Null { span: None };
                    }
                }
            }
            // Context-player member: `x`/`y`/`z` are reserved member names
            // that resolve unconditionally to the vector-component call; any
            // other member is a player-variable reference.
            if matches!(
                name.as_str(),
                "eventPlayer" | "hostPlayer" | "localPlayer" | "attacker" | "victim"
            ) {
                let player = context_player_expr(
                    name,
                    (!matches!(name.as_str(), "eventPlayer" | "hostPlayer"))
                        .then_some(receiver.span()),
                )
                .expect("context-player receiver name is exhaustive");
                if matches!(member, "x" | "y" | "z") {
                    return HirExpr::Member {
                        receiver: Box::new(player),
                        member: member.to_string(),
                        member_span: Some(member_span.into()),
                        span: Some(span.into()),
                    };
                }
                if !default_var_index(member).is_some() && !self.player_visible(member) {
                    let mut pool = crate::matcher::member_candidates(self.manifest, &self.catalog);
                    pool.extend(crate::matcher::bare_candidates(["x", "y", "z"]));
                    self.error_at_candidates(
                        "unknown-member",
                        format!("unknown member '{member}'"),
                        member_span,
                        member,
                        &pool,
                    );
                    return HirExpr::Null { span: None };
                }
                return HirExpr::PlayerVar {
                    player: Box::new(player),
                    name: member.to_string(),
                    member_span: Some(member_span.into()),
                    span: Some(span.into()),
                };
            }
            // A module member used without a call (`random.uniform` alone).
            if name == "random" {
                self.error_at(
                    "unsupported-member",
                    format!("module member '{name}.{member}' must be called"),
                    span,
                );
                return HirExpr::Null { span: None };
            }
            if self.player_visible(member) {
                return HirExpr::PlayerVar {
                    player: Box::new(self.lower_expr(receiver, macro_params, CallPosition::Value)),
                    name: member.to_string(),
                    member_span: Some(member_span.into()),
                    span: Some(span.into()),
                };
            }
            // A bare variable receiver member is valid OPY source syntax even
            // when canonical member existence is deferred to Workshop. Keep
            // both the resolved variable receiver and the source member
            // identity in HIR instead of treating it as an unknown member.
            if self.global_visible(name)
                || self.player_visible(name)
                || default_var_index(name).is_some()
            {
                let receiver = if default_var_index(name).is_some() {
                    HirExpr::GlobalVar {
                        name: name.to_string(),
                        span: Some(receiver.span().into()),
                    }
                } else {
                    self.lower_name(name, receiver.span(), &[])
                };
                return HirExpr::Member {
                    receiver: Box::new(receiver),
                    member: member.to_string(),
                    member_span: Some(member_span.into()),
                    span: Some(span.into()),
                };
            }
        }
        if self.player_visible(member) {
            return HirExpr::PlayerVar {
                player: Box::new(self.lower_expr(receiver, macro_params, CallPosition::Value)),
                name: member.to_string(),
                member_span: Some(member_span.into()),
                span: Some(span.into()),
            };
        }
        if matches!(member, "x" | "y" | "z") {
            return HirExpr::Member {
                receiver: Box::new(self.lower_expr(receiver, macro_params, CallPosition::Value)),
                member: member.to_string(),
                member_span: Some(member_span.into()),
                span: Some(span.into()),
            };
        }
        self.error_at(
            "unsupported-member",
            "unsupported member access on this expression".to_string(),
            span,
        );
        HirExpr::Null { span: None }
    }

    pub(super) fn lower_call(
        &mut self,
        name: &str,
        args: &[cst::CallArg],
        debug_source: Option<&str>,
        span: Span,
        macro_params: &[String],
        position: CallPosition,
    ) -> HirExpr {
        if matches!(name, "_" | "__" | "___") {
            if !matches!(args.len(), 1 | 2) {
                self.error_at(
                    "translations-invalid",
                    format!("translation function '{name}' expects one or two arguments"),
                    span,
                );
                return HirExpr::Null { span: None };
            }
            if args.len() == 2 && !matches!(args[0].value, cst::Expr::String { .. }) {
                self.error_at(
                    "translations-invalid",
                    "translation context must be a string literal".to_string(),
                    args[0].value.span(),
                );
                return HirExpr::Null { span: None };
            }
            return HirExpr::Call {
                name: name.to_string(),
                args: self.lower_arg_values(args, macro_params),
                debug_source: None,
                span: Some(span.into()),
            };
        }
        if name == "compressed" {
            return HirExpr::Call {
                name: name.to_string(),
                args: self.lower_arg_values(args, macro_params),
                debug_source: None,
                span: Some(span.into()),
            };
        }
        if let Some(special) = special_forms::SpecialValueCall::from_name(name) {
            return self.lower_special_call(special, name, args, span, macro_params);
        }
        // Builtin identity and position checks run before the generic call
        // arms so that a misplaced `wait`/`vect` still diagnoses its position.
        if !self.macro_visible(name) && !self.subroutine_visible(name) {
            match self.manifest.resolve_function(name) {
                Some(entry) => self.check_call_position(name, entry, position, span),
                None => {
                    let (code, message) = match position {
                        CallPosition::Statement => {
                            ("unknown-action", format!("unknown action '{name}'"))
                        }
                        CallPosition::Value => ("unknown-value", format!("unknown value '{name}'")),
                        CallPosition::ForIterable => (
                            "invalid-iterable",
                            format!("for-loop iterable '{name}' must be a range(...) call"),
                        ),
                        CallPosition::LambdaArgument | CallPosition::MacroBody => {
                            ("unknown-value", format!("unknown value '{name}'"))
                        }
                    };
                    let pool = match code {
                        "unknown-action" => {
                            crate::matcher::action_candidates(self.manifest, &self.catalog)
                        }
                        "unknown-value" => {
                            crate::matcher::value_candidates(self.manifest, &self.catalog)
                        }
                        _ => Vec::new(),
                    };
                    self.error_at_candidates(code, message, span, name, &pool);
                }
            }
        }
        match name {
            "vect" => {
                // `vect` goes through the generic argument binder so its
                // keyword forms (`vect(x=1, y=2, z=3)`) bind like any other
                // manifest signature; the result must fill exactly the three
                // declared parameters (x, y, z).
                let (bound, _) = match self.manifest.resolve_function(name) {
                    Some(entry) => self.bind_args(entry, args, macro_params),
                    None => (self.lower_arg_values(args, macro_params), None),
                };
                if bound.len() < 3 {
                    self.error_at(
                        "vect-arity",
                        format!(
                            "vect() expects 3 arguments (x, y, z) but got {}",
                            args.len()
                        ),
                        span,
                    );
                    return HirExpr::Null { span: None };
                }
                HirExpr::Vector {
                    x: Box::new(bound[0].clone()),
                    y: Box::new(bound[1].clone()),
                    z: Box::new(bound[2].clone()),
                    span: Some(span.into()),
                }
            }
            _ => {
                if self.macro_visible(name) {
                    // A declared `macro` invocation is recorded as a macroCall
                    // (positional-only; keyword arguments are an explicit
                    // diagnostic).
                    for arg in args {
                        if let Some((keyword, span)) = &arg.keyword {
                            self.error_at(
                                "keyword-unsupported",
                                format!(
                                    "macro '{name}' does not accept keyword \
                                     arguments ('{keyword}')"
                                ),
                                *span,
                            );
                        }
                    }
                    return HirExpr::MacroCall {
                        name: name.to_string(),
                        args: self.lower_arg_values(args, macro_params),
                        span: Some(span.into()),
                    };
                }
                if matches!(name, "async" | "startRule") {
                    let lowered = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            if index == 0 && arg.keyword.is_none() {
                                if let cst::Expr::Name {
                                    name: subroutine,
                                    span,
                                } = &arg.value
                                {
                                    if self.subroutine_visible(subroutine) {
                                        return HirExpr::Call {
                                            name: subroutine.clone(),
                                            args: Vec::new(),
                                            debug_source: None,
                                            span: Some((*span).into()),
                                        };
                                    }
                                }
                            }
                            self.lower_expr(&arg.value, macro_params, CallPosition::Value)
                        })
                        .collect();
                    return HirExpr::Call {
                        name: name.to_string(),
                        args: lowered,
                        debug_source: None,
                        span: Some(span.into()),
                    };
                }
                match self.manifest.resolve_function(name) {
                    Some(entry) => {
                        // Declared subroutines with arguments stay generic
                        // calls; builtins get keyword binding, arity, and
                        // domain/default handling.
                        if self.subroutine_visible(name) {
                            return HirExpr::Call {
                                name: name.to_string(),
                                args: self.lower_arg_values(args, macro_params),
                                debug_source: (name == "debug")
                                    .then(|| debug_source.map(str::to_string))
                                    .flatten(),
                                span: Some(span.into()),
                            };
                        }
                        let (bound, selector) = self.bind_args(entry, args, macro_params);
                        let (call_name, bound) =
                            self.resolve_contextual_domain(entry, bound, selector.as_deref());
                        HirExpr::Call {
                            name: call_name,
                            args: bound,
                            debug_source: (name == "debug")
                                .then(|| debug_source.map(str::to_string))
                                .flatten(),
                            span: Some(span.into()),
                        }
                    }
                    None => HirExpr::Call {
                        name: name.to_string(),
                        args: self.lower_arg_values(args, macro_params),
                        debug_source: None,
                        span: Some(span.into()),
                    },
                }
            }
        }
    }

    pub(super) fn lower_arg_values(
        &mut self,
        args: &[cst::CallArg],
        macro_params: &[String],
    ) -> Vec<HirExpr> {
        self.lower_arg_values_with_lambda(args, macro_params, |_, _| false)
    }

    pub(super) fn lower_arg_values_with_lambda(
        &mut self,
        args: &[cst::CallArg],
        macro_params: &[String],
        allows_lambda: impl Fn(usize, &cst::CallArg) -> bool,
    ) -> Vec<HirExpr> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                let position = if allows_lambda(index, arg) {
                    CallPosition::LambdaArgument
                } else {
                    CallPosition::Value
                };
                self.lower_expr(&arg.value, macro_params, position)
            })
            .collect()
    }

    /// Bind positional and keyword arguments against a manifest signature
    /// (issue #110), producing lowered values in parameter order with
    /// declared defaults filled. Diagnostics are structured and
    /// source-located: `unknown-keyword`, `duplicate-argument`,
    /// `keyword-required`, `positional-after-keyword`, `missing-argument`,
    /// `keyword-unsupported`, `invalid-arity` (overflow), and
    /// `invalid-argument` (the chase family's variable-argument requirement,
    /// typed policy at `policy::variable_args`).
    ///
    /// The returned `selector` is the keyword spelling used to bind the
    /// entry's contextual-domain selector parameter (the `chase` form's
    /// `rate`/`duration`), when the entry declares one.
    pub(super) fn bind_args(
        &mut self,
        entry: &Function,
        args: &[cst::CallArg],
        macro_params: &[String],
    ) -> (Vec<HirExpr>, Option<String>) {
        let mut slots: Vec<Option<HirExpr>> = vec![None; entry.params.len()];
        let mut selector = None;
        let mut has_keyword = false;
        let mut binding_error = false;
        let contextual = policy::contextual_domain(&entry.id);

        // Keyword spellings resolve through the declared parameter names
        // (alternate spellings included) — generic binding, no per-spelling
        // branches.
        let mut by_spelling: HashMap<&str, usize> = HashMap::new();
        for (index, param) in entry.params.iter().enumerate() {
            by_spelling.insert(param.name.as_str(), index);
            for alternate in &param.alternate_names {
                by_spelling.insert(alternate.as_str(), index);
            }
        }

        for (arg_index, arg) in args.iter().enumerate() {
            match &arg.keyword {
                Some((keyword, name_span)) => {
                    if !entry.keyword_args {
                        binding_error = true;
                        self.error_at(
                            "keyword-unsupported",
                            format!(
                                "function '{}' does not accept keyword arguments ('{keyword}')",
                                entry.id
                            ),
                            *name_span,
                        );
                        continue;
                    }
                    has_keyword = true;
                    match by_spelling.get(keyword.as_str()) {
                        None => {
                            binding_error = true;
                            self.error_at(
                                "unknown-keyword",
                                format!(
                                    "unknown keyword argument '{keyword}' for function '{}'",
                                    entry.id
                                ),
                                *name_span,
                            );
                        }
                        Some(&index) => {
                            let param = &entry.params[index];
                            if param.positional_only {
                                binding_error = true;
                                self.error_at(
                                    "unknown-keyword",
                                    format!(
                                        "parameter '{}' of '{}' cannot be bound by keyword",
                                        param.name, entry.id
                                    ),
                                    *name_span,
                                );
                            } else if slots[index].is_some() {
                                binding_error = true;
                                self.error_at(
                                    "duplicate-argument",
                                    format!(
                                        "argument '{}' of function '{}' is defined twice",
                                        keyword, entry.id
                                    ),
                                    *name_span,
                                );
                            } else {
                                slots[index] = Some(self.lower_call_arg_value(
                                    entry,
                                    index,
                                    arg,
                                    macro_params,
                                ));
                                if contextual.is_some_and(|c| c.by == param.name) {
                                    selector = Some(keyword.clone());
                                }
                            }
                        }
                    }
                }
                None => {
                    // The reference's generic binder rejects positional
                    // arguments after keyword arguments; its special forms
                    // (the contextual-domain entries, e.g. `chase`) bind the
                    // trailing positionals by slot and skip the ordering
                    // rule.
                    if has_keyword && contextual.is_none() {
                        binding_error = true;
                        self.error_at(
                            "positional-after-keyword",
                            format!(
                                "cannot use positional arguments after keyword \
                                 arguments in call to '{}'",
                                entry.id
                            ),
                            arg.value.span(),
                        );
                    }
                    // Positional arguments fill the slot at their argument
                    // index (keywords occupy their named slots), matching
                    // the reference binder.
                    let index = arg_index;
                    if index < entry.params.len() {
                        let param = &entry.params[index];
                        if param.keyword_only {
                            binding_error = true;
                            self.error_at(
                                "keyword-required",
                                format!(
                                    "argument {} of '{}' must be passed as a keyword \
                                     (name = value; accepted names: {})",
                                    index + 1,
                                    entry.id,
                                    keyword_spellings(param).join(", ")
                                ),
                                arg.value.span(),
                            );
                        }
                        if slots[index].is_none() {
                            slots[index] =
                                Some(self.lower_call_arg_value(entry, index, arg, macro_params));
                        }
                    } else {
                        self.lower_expr(&arg.value, macro_params, CallPosition::Value);
                    }
                }
            }
        }

        // Positional overflow: the declared arity bounds report the
        // reference's "takes N arguments, received M" rejection. A binding
        // error already reported (duplicate keyword, unknown keyword, …)
        // suppresses the secondary arity noise, matching the reference's
        // first-error behavior.
        if !binding_error && args.len() > entry.params.len() {
            self.check_arity(entry, args.len(), arg_span(args));
        }

        // Unbound parameters: declared defaults fill; required parameters
        // without a default are the reference's missing-argument rejection;
        // `optional` parameters stay omittable without an emitted expansion.
        let mut bound: Vec<HirExpr> = Vec::with_capacity(entry.params.len());
        for (index, param) in entry.params.iter().enumerate() {
            match &slots[index] {
                Some(value) => bound.push(value.clone()),
                None => match &param.default {
                    Some(ParamDefault::Call { call, args }) => {
                        bound.push(HirExpr::Call {
                            name: call.clone(),
                            args: args
                                .iter()
                                .map(|value| HirExpr::Number {
                                    value: *value,
                                    text: format!("{value}"),
                                    span: None,
                                })
                                .collect(),
                            debug_source: None,
                            span: None,
                        });
                    }
                    Some(ParamDefault::Null { .. }) => {
                        bound.push(HirExpr::Null { span: None });
                    }
                    Some(ParamDefault::EnumMember(member)) => {
                        let domain = param.domain.clone().unwrap_or_default();
                        bound.push(HirExpr::Enum {
                            value_type: domain,
                            value: member.clone(),
                            span: None,
                        });
                    }
                    Some(ParamDefault::Bool(value)) => {
                        bound.push(HirExpr::Bool {
                            value: *value,
                            span: None,
                        });
                    }
                    Some(ParamDefault::Number(number)) => {
                        bound.push(HirExpr::Number {
                            value: *number,
                            text: format!("{number}"),
                            span: None,
                        });
                    }
                    None if param.optional => {
                        // Omitted entirely (the reference's short forms keep
                        // the argument list short, e.g. `range(3)`).
                    }
                    None => {
                        self.error_at(
                            "missing-argument",
                            format!(
                                "missing argument '{}' for function '{}'",
                                param.name, entry.id
                            ),
                            arg_span(args),
                        );
                        bound.push(HirExpr::Null { span: None });
                    }
                },
            }
        }

        // Variable-required parameters (the chase family's first argument)
        // must resolve to a variable reference. The requirement is typed
        // policy keyed on the function id; the manifest's `param.variable`
        // flag records the same fact descriptively and does not select this
        // check (issue #458).
        for &index in policy::variable_args(&entry.id) {
            if let Some(Some(value)) = slots.get(index) {
                if !matches!(value, HirExpr::GlobalVar { .. } | HirExpr::PlayerVar { .. }) {
                    self.error_at(
                        "invalid-argument",
                        format!(
                            "argument {} of '{}' must be a variable (globalvar or \
                             playervar)",
                            index + 1,
                            entry.id
                        ),
                        arg_span(args),
                    );
                }
            }
        }

        (bound, selector)
    }

    /// Lower one call argument's value; the contextual-domain parameter (the
    /// `chase` form's `ChaseReeval` member) is recorded as a pending enum
    /// without validating the domain — it resolves only against the concrete
    /// domain selected by the call's keyword selector (issue #110). Outside
    /// that signature context `ChaseReeval` never resolves because it is not
    /// a declared enum domain.
    pub(super) fn lower_call_arg_value(
        &mut self,
        entry: &Function,
        param_index: usize,
        arg: &cst::CallArg,
        macro_params: &[String],
    ) -> HirExpr {
        if let Some(contextual) = policy::contextual_domain(&entry.id) {
            let is_contextual = entry.params[param_index]
                .domain
                .as_deref()
                .is_some_and(|domain| domain == contextual.domain);
            if is_contextual {
                if let Expr::Member {
                    receiver,
                    member,
                    span,
                    ..
                } = &arg.value
                {
                    if let Expr::Name { name, .. } = receiver.as_ref() {
                        if name == contextual.domain {
                            return HirExpr::Enum {
                                value_type: contextual.domain.to_string(),
                                value: member.clone(),
                                span: Some((*span).into()),
                            };
                        }
                    }
                }
            }
        }
        self.lower_expr(&arg.value, macro_params, CallPosition::Value)
    }

    /// Resolve a contextual enum-domain parameter (the `chase` form's
    /// `ChaseReeval` member, issue #110): the keyword spelling bound to the
    /// selector parameter selects the concrete domain and the function the
    /// call lowers to. This is pure catalog-identity dispatch — member
    /// *existence* in the selected domain is Workshop-owned knowledge and is
    /// not validated here (lowering-dependent, issue #8). Outside this
    /// signature context `ChaseReeval` never resolves (it is not a standalone
    /// domain identity). A call that does not bind a contextual member keeps
    /// its original function identity.
    pub(super) fn resolve_contextual_domain(
        &mut self,
        entry: &Function,
        mut bound: Vec<HirExpr>,
        selector: Option<&str>,
    ) -> (String, Vec<HirExpr>) {
        let Some(contextual) = policy::contextual_domain(&entry.id) else {
            return (entry.id.clone(), bound);
        };
        let Some(contextual_param) = entry
            .params
            .iter()
            .position(|param| param.domain.as_deref() == Some(contextual.domain))
        else {
            return (entry.id.clone(), bound);
        };
        // Dispatch only when the argument is written as a member of the
        // contextual domain; anything else keeps the generic call (the
        // reference's "expected an enum" rejection is lowering-dependent).
        let HirExpr::Enum {
            value_type,
            value,
            span: value_span,
        } = &bound[contextual_param]
        else {
            return (entry.id.clone(), bound);
        };
        if value_type != contextual.domain {
            return (entry.id.clone(), bound);
        }
        let Some(keyword) = selector else {
            return (entry.id.clone(), bound);
        };
        let Some(option) = contextual
            .options
            .iter()
            .find(|option| option.keyword == keyword)
        else {
            return (entry.id.clone(), bound);
        };
        bound[contextual_param] = HirExpr::Enum {
            value_type: option.domain.to_string(),
            value: value.clone(),
            span: *value_span,
        };
        (option.target.to_string(), bound)
    }

    pub(super) fn lower_receiver_call(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[cst::CallArg],
        span: Span,
        macro_params: &[String],
        position: CallPosition,
    ) -> HirExpr {
        if let Some(macro_name) = self.qualified_macro_for_member(name) {
            return HirExpr::MacroCall {
                name: macro_name,
                args: std::iter::once(self.lower_expr(receiver, macro_params, CallPosition::Value))
                    .chain(self.lower_arg_values(args, macro_params))
                    .collect(),
                span: Some(span.into()),
            };
        }
        if name == "getOppositeTeam" {
            return HirExpr::ReceiverCall {
                receiver: Box::new(self.lower_expr(receiver, macro_params, CallPosition::Value)),
                name: "getOppositeTeam".to_string(),
                args: self.lower_arg_values(args, macro_params),
                span: Some(span.into()),
            };
        }
        if name == "toArray" {
            if let Expr::Name {
                name: type_name, ..
            } = receiver
            {
                if self.manifest.domain_identity(type_name)
                    || self.catalog.enum_domain(type_name).is_some()
                {
                    return HirExpr::ReceiverCall {
                        receiver: Box::new(HirExpr::Type {
                            name: type_name.clone(),
                            args: Vec::new(),
                            span: Some(receiver.span().into()),
                        }),
                        name: name.to_string(),
                        args: self.lower_arg_values(args, macro_params),
                        span: Some(span.into()),
                    };
                }
            }
        }
        if matches!(name, "map" | "filter" | "all" | "any") {
            // The reference parser special-cases these member calls to a bare
            // `lambda x: expr` argument; keyword spellings cannot be written.
            for arg in args {
                if let Some((keyword, span)) = &arg.keyword {
                    self.error_at(
                        "keyword-unsupported",
                        format!(
                            "function '{name}' does not accept keyword arguments ('{keyword}')"
                        ),
                        *span,
                    );
                }
            }
            let lowered = HirExpr::ReceiverCall {
                receiver: Box::new(self.lower_expr(receiver, macro_params, CallPosition::Value)),
                name: name.to_string(),
                args: self.lower_arg_values_with_lambda(args, macro_params, |index, _| index == 0),
                span: Some(span.into()),
            };
            return lowered;
        }
        // `random.uniform(...)` etc. are dotted generic calls.
        if let Expr::Name { name: root, .. } = receiver {
            if root == "random" {
                return self.lower_call(
                    &format!("random.{name}"),
                    args,
                    None,
                    span,
                    macro_params,
                    position,
                );
            }
        }
        // `.format` on a string literal is the format special form; it is
        // also a declared member value, so position misuse diagnoses here.
        if let Expr::String { value, .. } = receiver {
            if name == "format" {
                if args.iter().any(|arg| arg.keyword.is_some()) {
                    for arg in args {
                        if let Some((keyword, span)) = &arg.keyword {
                            self.error_at(
                                "keyword-unsupported",
                                format!(
                                    "function 'format' does not accept keyword \
                                     arguments ('{keyword}')"
                                ),
                                *span,
                            );
                        }
                    }
                }
                let lowered: Vec<HirExpr> = self.lower_arg_values(args, macro_params);
                if let Some(entry) = self.manifest.resolve_member("format") {
                    self.check_call_position("format", entry, position, span);
                }
                return HirExpr::Format {
                    text: value.clone(),
                    args: lowered,
                    span: Some(span.into()),
                };
            }
        }
        // Member calls resolve through the manifest (identity,
        // explicit-argument signatures, keyword binding); enforced receiver
        // requirements are typed member policy, not manifest metadata.
        let (member_name, lowered) = match self.manifest.resolve_member(name) {
            Some(entry) => {
                self.check_call_position(name, entry, position, span);
                self.check_receiver(receiver, entry, span);
                let (bound, _) = self.bind_args(entry, args, macro_params);
                (entry.id.clone(), bound)
            }
            None => {
                self.error_at_candidates(
                    "unknown-member",
                    format!("unknown member '{name}'"),
                    span,
                    name,
                    &crate::matcher::member_candidates(self.manifest, &self.catalog),
                );
                (name.to_string(), self.lower_arg_values(args, macro_params))
            }
        };
        // `eventPlayer.member(...)` → receiver call on the event player.
        if let Expr::Name { name: root, .. } = receiver {
            if matches!(
                root.as_str(),
                "eventPlayer" | "hostPlayer" | "localPlayer" | "attacker" | "victim"
            ) {
                return HirExpr::ReceiverCall {
                    receiver: Box::new(
                        context_player_expr(
                            root,
                            (!matches!(root.as_str(), "eventPlayer" | "hostPlayer"))
                                .then_some(receiver.span()),
                        )
                        .expect("context-player receiver name is exhaustive"),
                    ),
                    name: member_name,
                    args: lowered,
                    span: Some(span.into()),
                };
            }
        }
        // Any other receiver: resolve it and keep the receiver call.
        HirExpr::ReceiverCall {
            receiver: Box::new(self.lower_expr(receiver, macro_params, CallPosition::Value)),
            name: member_name,
            args: lowered,
            span: Some(span.into()),
        }
    }

    /// Check a builtin entry against its call position: action/value
    /// identity and for-iterable context.
    pub(super) fn check_call_position(
        &mut self,
        name: &str,
        entry: &Function,
        position: CallPosition,
        span: Span,
    ) {
        match position {
            CallPosition::Statement => {
                if policy::function_context(&entry.id) == Some(policy::FunctionContext::ForIterable)
                {
                    self.error_at(
                        "invalid-call-context",
                        format!("'{name}' is only valid as a for-loop iterable"),
                        span,
                    );
                } else if entry.kind.is_value() {
                    self.error_at(
                        "value-in-action-position",
                        format!("value function '{name}' cannot be used as an action"),
                        span,
                    );
                }
            }
            CallPosition::Value | CallPosition::LambdaArgument => {
                if entry.kind.is_action() {
                    self.error_at(
                        "action-in-value-position",
                        format!("action function '{name}' cannot be used as a value"),
                        span,
                    );
                } else if policy::function_context(&entry.id)
                    == Some(policy::FunctionContext::ForIterable)
                {
                    self.error_at(
                        "invalid-call-context",
                        format!("'{name}' is only valid as a for-loop iterable"),
                        span,
                    );
                }
            }
            CallPosition::ForIterable => {
                if policy::function_context(&entry.id) != Some(policy::FunctionContext::ForIterable)
                {
                    self.error_at(
                        "invalid-iterable",
                        format!("for-loop iterable '{name}' must be a range(...) call"),
                        span,
                    );
                }
            }
            CallPosition::MacroBody => {}
        }
    }

    /// Check a member call's receiver against the requirement the member
    /// enforces (`policy::member_receiver_requirement`): `.append` and
    /// `.remove` require an assignable receiver, `.format` a string literal;
    /// all other members accept any receiver (the pinned reference does not
    /// type-check player-oriented receivers). The requirement is typed
    /// policy keyed on the member id — the manifest's `Function::receiver`
    /// category is descriptive signature metadata and does not select this
    /// check (issue #458).
    pub(super) fn check_receiver(&mut self, receiver: &Expr, entry: &Function, span: Span) {
        let Some(requirement) = policy::member_receiver_requirement(&entry.id) else {
            return;
        };
        let mismatch = match requirement {
            policy::ReceiverRequirement::StringLiteral => {
                !matches!(receiver, Expr::String { .. } | Expr::StringModifier { .. })
            }
            policy::ReceiverRequirement::Assignable => !assignable_receiver(receiver),
        };
        if mismatch {
            self.error_at(
                "invalid-receiver",
                format!(
                    "member '{}' requires {} as its receiver",
                    entry.id,
                    requirement.describe()
                ),
                span,
            );
        }
    }

    /// Check a builtin call's argument count against its declared arity.
    pub(super) fn check_arity(&mut self, entry: &Function, got: usize, span: Span) {
        let (min, max) = entry.arity_bounds();
        let valid = got >= min && max.is_none_or(|max| got <= max);
        if !valid {
            let expects = match max {
                Some(max) if min == max => format!("exactly {min}"),
                Some(max) => format!("{min} to {max}"),
                None => format!("at least {min}"),
            };
            let role = match entry.kind {
                FunctionKind::Action => "action",
                FunctionKind::Value => "value",
                FunctionKind::MemberAction => "member action",
                FunctionKind::MemberValue => "member value",
            };
            self.error_at(
                "invalid-arity",
                format!(
                    "{role} '{}' expects {expects} arguments but got {got}",
                    entry.id
                ),
                span,
            );
        }
    }
}

impl Lowerer {
    fn lower_string(&mut self, value: &str, span: Span) -> HirExpr {
        if !self.setup_tags {
            return HirExpr::String {
                value: value.to_string(),
                span: Some(span.into()),
            };
        }
        let mut text = value.to_string();
        let mut tagged = false;
        for (source, replacement) in [
            ("<fg", "{0}fg"),
            ("</fg>", "{0}/fg>"),
            ("<tx", "{0}tx"),
            ("<TX", "{0}TX"),
            ("</tx>", "{0}/tx>"),
            ("</TX>", "{0}/TX>"),
        ] {
            if text.contains(source) {
                text = text.replace(source, replacement);
                tagged = true;
            }
        }
        if !tagged {
            return HirExpr::String {
                value: value.to_string(),
                span: Some(span.into()),
            };
        }
        self.texture_used = true;
        HirExpr::Format {
            text,
            args: vec![HirExpr::GlobalVar {
                name: "__holygrail__".to_string(),
                span: Some(span.into()),
            }],
            span: Some(span.into()),
        }
    }
}
