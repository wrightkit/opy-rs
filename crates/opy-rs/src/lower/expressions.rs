//! Expression, builtin, argument, and member resolution.

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
            Expr::String { value, span } => HirExpr::String {
                value: value.clone(),
                span: Some(span.into()),
            },
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
            Expr::Lambda { params, body, span } => {
                if position != CallPosition::LambdaArgument {
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
            Expr::Call { name, args, span } => {
                self.lower_call(name, args, *span, macro_params, position)
            }
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
                span: Some(span.into()),
            },
            "eventAbility" | "eventDamage" | "eventHealing" | "eventWasCriticalHit" => {
                HirExpr::Call {
                    name: name.to_string(),
                    args: Vec::new(),
                    span: Some(span.into()),
                }
            }
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
            // Custom enum member: folds to its numeric constant.
            if self.enum_visible(name) {
                let members = self.enums.get(name).expect("enum span and members agree");
                return match members.iter().position(|candidate| candidate == member) {
                    Some(index) => HirExpr::Number {
                        value: index as f64,
                        text: index.to_string(),
                        span: Some(span.into()),
                    },
                    None => {
                        self.error_at(
                            "unknown-enum-member",
                            format!("enum '{name}' has no member '{member}'"),
                            span,
                        );
                        HirExpr::Null { span: None }
                    }
                };
            }
            if name == "Math" {
                if let Some((value, text)) = match member {
                    "PI" => Some((std::f64::consts::PI, "3.141592653589793")),
                    "E" => Some((std::f64::consts::E, "2.718281828459045")),
                    "INFINITY" => Some((999_999_999_999.0, "999999999999")),
                    "EPSILON" => Some((1192093e-13, "0.0000001192093")),
                    _ => None,
                } {
                    return HirExpr::Number {
                        value,
                        text: text.to_string(),
                        span: Some(span.into()),
                    };
                }
            }
            // Builtin Workshop enum: the domain name is a declared OPY
            // signature identity (manifest `param.domain`); the member list
            // is Workshop-owned catalog content, so the member access
            // resolves as an opaque identity after validating the member
            // against the canonical Workshop catalog.
            let catalog_domain = match name.as_str() {
                "Clip" => "Clipping",
                "AsyncBehavior" => "StartRuleBehavior",
                _ => name.as_str(),
            };
            if self.manifest.domain_identity(name)
                || self.catalog.enum_domain(name).is_some()
                || self.catalog.enum_domain(catalog_domain).is_some()
                || (name == "Clip" && self.manifest.domain_identity(catalog_domain))
            {
                let locale = Locale::new("en-US");
                let catalog_member = match (name.as_str(), member) {
                    ("Map", "BLIZZ_WORLD") => "BLIZZARD_WORLD",
                    ("Map", "BLIZZ_WORLD_WINTER") => "BLIZZARD_WORLD_WINTER",
                    ("Map", "ROUTE66") => "ROUTE_66",
                    ("Map", "VOLSKAYA") => "VOLSKAYA_INDUSTRIES",
                    ("Clip", "NONE") => "DO_NOT_CLIP",
                    ("Clip", "SURFACES") => "CLIP_AGAINST_SURFACES",
                    ("SpecVisibility", "ALWAYS") => "VISIBLE_ALWAYS",
                    ("SpecVisibility", "NEVER") => "VISIBLE_NEVER",
                    ("EffectReeval", "VISIBILITY_POSITION_AND_RADIUS") => {
                        "VISIBLE_TO_POSITION_AND_RADIUS"
                    }
                    ("HudReeval", "VISIBILITY_AND_COLOR") => "VISIBLE_TO_AND_COLOR",
                    ("HudReeval", "VISIBILITY_STRING_AND_COLOR") => "VISIBLE_TO_STRING_AND_COLOR",
                    ("Hero", "MCCREE") => "CASSIDY",
                    ("Hero", "HAMMOND") => "WRECKING_BALL",
                    ("Hero", "SOLDIER") => "SOLDIER_76",
                    ("Hero", "DOMINA") => "JINYU",
                    ("Hero", "DMON") => "D_MON",
                    _ => member,
                };
                let canonical_member = self
                    .catalog
                    .enum_domain(catalog_domain)
                    .and_then(|domain| {
                        domain
                            .members
                            .iter()
                            .find(|candidate| candidate.member == catalog_member)
                            .map(|candidate| candidate.member.clone())
                            .or_else(|| {
                                (name == "Map").then(|| {
                                    let normalized = catalog_member.replace('_', "");
                                    domain
                                        .members
                                        .iter()
                                        .find(|candidate| {
                                            candidate.member.replace('_', "") == normalized
                                        })
                                        .map(|candidate| candidate.member.clone())
                                })?
                            })
                    })
                    .or_else(|| {
                        if name == "Team" && member.parse::<u32>().is_ok() {
                            self.catalog
                                .resolve_enum_member(name, &locale, &format!("{name} {member}"))
                                .map(|(_, member)| member)
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        (name == "HudReeval" && member == "VISIBILITY_STRING_AND_COLOR")
                            .then_some(catalog_member.to_string())
                    });
                let Some(canonical_member) = canonical_member else {
                    self.error_at(
                        "unknown-enum-member",
                        format!("enum '{name}' has no member '{member}'"),
                        span,
                    );
                    return HirExpr::Null { span: None };
                };
                return HirExpr::Enum {
                    value_type: catalog_domain.to_string(),
                    value: canonical_member,
                    span: Some(span.into()),
                };
            }
            // Context-player member: a player-variable reference.
            if matches!(
                name.as_str(),
                "eventPlayer" | "hostPlayer" | "localPlayer" | "attacker" | "victim"
            ) {
                if !default_var_index(member).is_some() && !self.player_visible(member) {
                    self.error_at(
                        "unknown-member",
                        format!("unknown member '{member}'"),
                        member_span,
                    );
                    return HirExpr::Null { span: None };
                }
                let player = context_player_expr(
                    name,
                    (!matches!(name.as_str(), "eventPlayer" | "hostPlayer"))
                        .then_some(receiver.span()),
                )
                .expect("context-player receiver name is exhaustive");
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
        span: Span,
        macro_params: &[String],
        position: CallPosition,
    ) -> HirExpr {
        if name == "createWorkshopSetting" {
            return self.lower_workshop_setting(args, span, macro_params);
        }
        if name == "compressed" && args.len() == 1 {
            return self.lower_expr(&args[0].value, macro_params, CallPosition::Value);
        }
        if matches!(
            name,
            "createWorkshopSettingBool"
                | "createWorkshopSettingEnum"
                | "createWorkshopSettingInt"
                | "createWorkshopSettingFloat"
        ) {
            return HirExpr::Call {
                name: name.to_string(),
                args: self.lower_arg_values(args, macro_params),
                span: Some(span.into()),
            };
        }
        // Builtin identity and position checks run before the special forms
        // so that a misplaced `wait`/`vect` still diagnoses its position.
        if !self.macro_visible(name) && !self.subroutine_visible(name) && name != "sorted" {
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
                        CallPosition::LambdaArgument => {
                            ("unknown-value", format!("unknown value '{name}'"))
                        }
                        CallPosition::MacroBody => {
                            ("unknown-value", format!("unknown value '{name}'"))
                        }
                    };
                    self.error_at(code, message, span);
                }
            }
        }
        match name {
            "sorted" => HirExpr::Call {
                name: name.to_string(),
                args: self.lower_arg_values_with_lambda(args, macro_params, |index, arg| {
                    index == 1 || arg.keyword.as_ref().is_some_and(|(name, _)| name == "key")
                }),
                span: Some(span.into()),
            },
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
                if name == "async" {
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
                                span: Some(span.into()),
                            };
                        }
                        let (bound, selector) = self.bind_args(entry, args, macro_params);
                        let (call_name, bound) =
                            self.resolve_contextual_domain(entry, bound, selector.as_deref());
                        HirExpr::Call {
                            name: call_name,
                            args: bound,
                            span: Some(span.into()),
                        }
                    }
                    None => HirExpr::Call {
                        name: name.to_string(),
                        args: self.lower_arg_values(args, macro_params),
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
    /// `invalid-argument` (variable-required parameters).
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
        let contextual = entry.contextual_domain.as_ref();

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
                    if has_keyword && entry.contextual_domain.is_none() {
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
                    Some(ParamDefault::Call { call }) => {
                        bound.push(HirExpr::Call {
                            name: call.clone(),
                            args: Vec::new(),
                            span: None,
                        });
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
        // must resolve to a variable reference.
        for (index, param) in entry.params.iter().enumerate() {
            if !param.variable {
                continue;
            }
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
        if let Some(contextual) = &entry.contextual_domain {
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
                        if name == &contextual.domain {
                            return HirExpr::Enum {
                                value_type: contextual.domain.clone(),
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
        let Some(contextual) = &entry.contextual_domain else {
            return (entry.id.clone(), bound);
        };
        let Some(contextual_param) = entry
            .params
            .iter()
            .position(|param| param.domain.as_deref() == Some(contextual.domain.as_str()))
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
        if value_type != &contextual.domain {
            return (entry.id.clone(), bound);
        }
        let Some(keyword) = selector else {
            return (entry.id.clone(), bound);
        };
        let Some(option) = contextual.options.get(keyword) else {
            return (entry.id.clone(), bound);
        };
        bound[contextual_param] = HirExpr::Enum {
            value_type: option.domain.clone(),
            value: value.clone(),
            span: *value_span,
        };
        (option.target.clone(), bound)
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
        if matches!(name, "map" | "filter" | "all" | "any") {
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
                    span,
                    macro_params,
                    position,
                );
            }
        }
        // `.format` on a string literal is the format special form; it is
        // also a declared member value (receiver category `String`), so
        // position misuse diagnoses here.
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
        // Member calls resolve through the manifest (receiver category,
        // explicit-argument signatures, keyword binding).
        let (member_name, lowered) = match self.manifest.resolve_member(name) {
            Some(entry) => {
                self.check_call_position(name, entry, position, span);
                if let Some(category) = entry.receiver {
                    self.check_receiver(receiver, category, entry, span);
                }
                let (bound, _) = self.bind_args(entry, args, macro_params);
                (entry.id.clone(), bound)
            }
            None => {
                self.error_at("unknown-member", format!("unknown member '{name}'"), span);
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
                if entry.context == Some(FunctionContext::ForIterable) {
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
            CallPosition::Value => {
                if entry.kind.is_action() {
                    self.error_at(
                        "action-in-value-position",
                        format!("action function '{name}' cannot be used as a value"),
                        span,
                    );
                } else if entry.context == Some(FunctionContext::ForIterable) {
                    self.error_at(
                        "invalid-call-context",
                        format!("'{name}' is only valid as a for-loop iterable"),
                        span,
                    );
                }
            }
            CallPosition::ForIterable => {
                if entry.context != Some(FunctionContext::ForIterable) {
                    self.error_at(
                        "invalid-iterable",
                        format!("for-loop iterable '{name}' must be a range(...) call"),
                        span,
                    );
                }
            }
            CallPosition::LambdaArgument => {
                if entry.kind.is_action() {
                    self.error_at(
                        "action-in-value-position",
                        format!("action function '{name}' cannot be used as a value"),
                        span,
                    );
                } else if entry.context == Some(FunctionContext::ForIterable) {
                    self.error_at(
                        "invalid-call-context",
                        format!("'{name}' is only valid as a for-loop iterable"),
                        span,
                    );
                }
            }
            CallPosition::MacroBody => {}
        }
    }

    /// Check a member call's receiver against its declared category. Only
    /// the reference-enforced categories reject: `.append` requires an
    /// assignable receiver and `.format` a string literal; player-oriented
    /// members accept any receiver (the pinned reference does not type-check
    /// them).
    pub(super) fn check_receiver(
        &mut self,
        receiver: &Expr,
        category: ReceiverCategory,
        entry: &Function,
        span: Span,
    ) {
        let mismatch = match category {
            ReceiverCategory::String => {
                !matches!(receiver, Expr::String { .. } | Expr::StringModifier { .. })
            }
            ReceiverCategory::Variable => !assignable_receiver(receiver),
            ReceiverCategory::Player | ReceiverCategory::Vector | ReceiverCategory::Any => false,
        };
        if mismatch {
            self.error_at(
                "invalid-receiver",
                format!(
                    "member '{}' requires {} as its receiver",
                    entry.id,
                    category.describe()
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
