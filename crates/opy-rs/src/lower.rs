//! Semantic resolution and HIR lowering (#45).
//!
//! Resolves the parsed CST into the opy-rs-owned Opy HIR contract
//! ([`crate::hir::Program`]): declarations and references resolve to typed
//! HIR nodes, custom enums fold to constants, `vect` becomes a vector,
//! `.format()` becomes a format node, `wait` default arguments are filled,
//! and subroutine calls become `CallSubroutine` statements. Semantic errors
//! (unknown identifiers, unknown custom-enum members, invalid `vect` arity)
//! are structured and source-located.
//!
//! Builtin action/value/member identity, action/value position, signatures
//! and arity, receiver categories, parameter enum-domain identities, and
//! non-contextual source aliases resolve through the OPY semantic
//! compatibility manifest ([`crate::manifest`], issue #109) before Workshop
//! emission: unknown or misplaced builtins fail here with structured,
//! source-located diagnostics instead of surfacing as emitter catalog
//! misses.
//!
//! Ownership boundary: enum *domains* are catalog-identity links carried by
//! the manifest signatures, but enum *member lists* are Workshop-owned
//! catalog content. A member access on a declared domain identity resolves
//! as an opaque `Enum` node and member-existence/domain checks
//! (`unknown-enum-member` for Workshop enums, `enum-domain-mismatch`) are
//! `lowering-dependent` (issue #8) — they are not approximated here. Custom
//! (user-declared) enum member checks are OPY-level source semantics and
//! stay in this frontend.

use std::collections::{HashMap, HashSet};

use crate::hir::types::{
    Annotation as HirAnnotation, AnnotationArg as HirAnnotationArg, Declaration, Define,
    DictEntry as HirDictEntry, Event, Expr as HirExpr, Generator, IfBranch, PROTOCOL_VERSION,
    Position, PreprocessingState, Program as HirProgram, Protocol, Rule, RuleEntry,
    Settings as HirSettings, SettingsNode as HirSettingsNode, SourceFile, Span as HirSpan,
    Stmt as HirStmt, SwitchArm as HirSwitchArm, default_var_index,
};

use crate::cst::{self, CallArg, Decl, Expr, RuleEntry as CstRuleEntry, Stmt};
use crate::diag::{OpyError, OpyResult, Span};
use crate::manifest::{
    Function, FunctionContext, FunctionKind, Manifest, Param, ParamDefault, ReceiverCategory,
};
use workshop_rs::catalog::{Catalog, Locale};

/// The protocol envelope this frontend produces.
const PROTOCOL_NAME: &str = "wright/opy-hir";

/// The call-position context of an expression being lowered; builtin
/// resolution checks action/value identity against this context.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallPosition {
    /// A statement position (a bare expression statement).
    Statement,
    /// A value position (conditions, assignments, call arguments, …).
    Value,
    /// A `for ... in` iterable (only `range` is a valid builtin here).
    ForIterable,
    /// An expression occupying a signature-approved lambda argument slot.
    LambdaArgument,
}

/// The lowerer's symbol context, built from the CST declarations.
struct Lowerer {
    global_declarations: HashMap<String, usize>,
    player_declarations: HashMap<String, usize>,
    subroutine_declarations: HashMap<String, usize>,
    subroutine_definitions: Vec<(String, usize)>,
    macro_declarations: HashMap<String, usize>,
    enums: HashMap<String, Vec<String>>,
    enum_declarations: HashMap<String, usize>,
    locals: Vec<String>,
    current_order: usize,
    allow_dict_literal: bool,
    /// The authoritative builtin semantic table (issue #109).
    manifest: &'static Manifest,
    /// The canonical Workshop catalog linked by the manifest.
    catalog: Catalog,
    errors: Vec<OpyError>,
}

/// Lower a parsed program into the Opy HIR contract.
pub fn lower(
    program: &cst::Program,
    files: Vec<SourceFile>,
    defines: Vec<Define>,
) -> OpyResult<HirProgram> {
    lower_with_preprocessing(program, files, defines, &PreprocessingState::default())
}

pub fn lower_with_preprocessing(
    program: &cst::Program,
    files: Vec<SourceFile>,
    defines: Vec<Define>,
    preprocessing: &PreprocessingState,
) -> OpyResult<HirProgram> {
    let manifest = match Manifest::builtin() {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(OpyError::new(
                "manifest-error",
                format!("cannot load the OPY semantic compatibility manifest: {error}"),
            ));
        }
    };
    let catalog = match Catalog::builtin() {
        Ok(catalog) => catalog,
        Err(error) => {
            return Err(OpyError::new(
                "catalog-error",
                format!("cannot load the Workshop catalog: {error}"),
            ));
        }
    };
    let mut lowerer = Lowerer {
        global_declarations: HashMap::new(),
        player_declarations: HashMap::new(),
        subroutine_declarations: HashMap::new(),
        subroutine_definitions: Vec::new(),
        macro_declarations: HashMap::new(),
        enums: HashMap::new(),
        enum_declarations: HashMap::new(),
        locals: Vec::new(),
        current_order: 0,
        allow_dict_literal: false,
        manifest,
        catalog,
        errors: Vec::new(),
    };
    lowerer.collect_symbols(program);

    let mut declarations = Vec::new();
    for (order, decl) in program.declarations.iter().enumerate() {
        lowerer.current_order = order;
        match decl {
            Decl::GlobalVariable {
                name,
                index,
                span,
                name_span,
                initializer,
            } => {
                declarations.push(Declaration::GlobalVariable {
                    name: name.clone(),
                    index: *index,
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                    initializer: lowerer.initializer(initializer.as_ref()),
                });
            }
            Decl::PlayerVariable {
                name,
                index,
                span,
                name_span,
                initializer,
            } => {
                declarations.push(Declaration::PlayerVariable {
                    name: name.clone(),
                    index: *index,
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                    initializer: lowerer.initializer(initializer.as_ref()),
                });
            }
            Decl::Subroutine {
                name,
                span,
                name_span,
            } => {
                declarations.push(Declaration::Subroutine {
                    name: name.clone(),
                    index: None,
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                });
            }
            Decl::Enum { .. } => {
                // Custom enums fold to numeric constants at use sites and
                // produce no HIR declaration (reference behavior).
            }
            Decl::Macro {
                name,
                args,
                body,
                span,
            } => {
                let lowered_body = lowerer.lower_macro_body(body, args);
                declarations.push(Declaration::Macro {
                    name: name.clone(),
                    args: args.clone(),
                    span: Some(span.into()),
                    body: lowered_body,
                });
            }
        }
    }

    let mut implicit_subroutines = HashSet::new();
    for entry in &program.rules {
        let CstRuleEntry::SubroutineDef {
            name,
            span,
            name_span,
            ..
        } = entry
        else {
            continue;
        };
        if !lowerer.subroutine_declarations.contains_key(name)
            && implicit_subroutines.insert(name.clone())
        {
            declarations.push(Declaration::Subroutine {
                name: name.clone(),
                index: None,
                span: Some(span.into()),
                name_span: Some(name_span.into()),
            });
        }
    }
    let mut rules = Vec::new();
    for (rule_order, entry) in program.rules.iter().enumerate() {
        lowerer.current_order = program.declarations.len() + rule_order;
        match entry {
            CstRuleEntry::Rule(rule) => rules.push(RuleEntry::Rule(lowerer.lower_rule(
                rule,
                files.as_slice(),
                preprocessing,
            )?)),
            CstRuleEntry::SubroutineDef {
                name,
                presentation_name,
                span,
                name_span,
                body,
                annotations,
                rule_prefix,
            } => {
                let base_name = presentation_name
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| name.clone());
                let generated_name = render_rule_name(
                    &base_name,
                    rule_prefix.as_deref(),
                    false,
                    *span,
                    files.as_slice(),
                    preprocessing,
                )?;
                rules.push(RuleEntry::SubroutineDef {
                    kind: "subroutineDef".to_string(),
                    name: generated_name,
                    source_name: name.clone(),
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                    body: lowerer.lower_block(body, &[], false, true, false),
                    annotations: lower_annotations(annotations),
                });
            }
        }
    }

    if !lowerer.errors.is_empty() {
        return Err(lowerer.errors.swap_remove(0));
    }

    Ok(HirProgram {
        protocol: Protocol {
            name: PROTOCOL_NAME.to_string(),
            version: PROTOCOL_VERSION.to_string(),
        },
        generator: Generator {
            name: crate::LANGUAGE_NAME.to_string(),
            version: crate::LANGUAGE_VERSION.to_string(),
            frontend: crate::LANGUAGE_NAME.to_string(),
        },
        files,
        defines,
        declarations,
        rules,
        settings: program.settings.as_ref().map(lower_settings),
        preprocessing: preprocessing.clone(),
    })
}

fn prefixed_rule_name(name: &str, prefix: Option<&str>, delimiter: bool) -> String {
    match prefix {
        Some(prefix) if !prefix.is_empty() && !delimiter && !name.is_empty() => {
            format!("[{prefix}] {name}")
        }
        _ => name.to_string(),
    }
}

#[derive(Clone, Debug)]
enum TemplateValue {
    String(String),
    Bool(bool),
}

fn render_rule_name(
    name: &str,
    prefix: Option<&str>,
    delimiter: bool,
    span: Span,
    files: &[SourceFile],
    preprocessing: &PreprocessingState,
) -> OpyResult<String> {
    let Some(template) = preprocessing
        .rule_prefix_template
        .as_ref()
        .map(|value| value.value.as_str())
    else {
        return Ok(prefixed_rule_name(name, prefix, delimiter));
    };
    let (file, path) = rule_file_parts(span.file, files);
    let prefix = prefix.unwrap_or_default();
    let values = [
        ("$rule", TemplateValue::String(name.to_string())),
        ("$prefix", TemplateValue::String(prefix.to_string())),
        ("$file", TemplateValue::String(file.clone())),
        ("$path", TemplateValue::String(path.clone())),
        ("$isDelimiter", TemplateValue::Bool(delimiter)),
        ("$prefixTitle", TemplateValue::String(title_case(prefix))),
        ("$prefixUpper", TemplateValue::String(prefix.to_uppercase())),
        ("$prefixLower", TemplateValue::String(prefix.to_lowercase())),
        ("$fileTitle", TemplateValue::String(title_case(&file))),
        ("$fileUpper", TemplateValue::String(file.to_uppercase())),
        ("$fileLower", TemplateValue::String(file.to_lowercase())),
        ("$pathTitle", TemplateValue::String(title_case(&path))),
        ("$pathUpper", TemplateValue::String(path.to_uppercase())),
        ("$pathLower", TemplateValue::String(path.to_lowercase())),
    ];
    evaluate_template(template, &values).map_err(|message| {
        OpyError::at(
            "rule-prefix-template-invalid",
            format!("could not resolve rule prefix template: {message}"),
            span,
        )
    })
}

fn rule_file_parts(file_id: u32, files: &[SourceFile]) -> (String, String) {
    let path = files
        .iter()
        .find(|file| file.id == file_id)
        .map(|file| file.path.replace('\\', "/"))
        .unwrap_or_default();
    let without_extension = path
        .strip_suffix(".opy")
        .or_else(|| path.strip_suffix(".OPY"))
        .unwrap_or(&path)
        .to_string();
    let file = without_extension
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    (file, without_extension)
}

fn title_case(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut capitalize = true;
    for ch in value.chars() {
        if ch == '_' {
            result.push(' ');
            capitalize = true;
        } else if capitalize && ch.is_ascii_alphabetic() {
            result.push(ch.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(ch);
            if !ch.is_whitespace() && ch != '/' {
                capitalize = false;
            }
        }
        if ch == '/' || ch.is_whitespace() {
            capitalize = true;
        }
    }
    result
}

fn evaluate_template(template: &str, values: &[(&str, TemplateValue)]) -> Result<String, String> {
    if let Some((then_value, condition, else_value)) = split_conditional(template) {
        let branch = if evaluate_condition(condition, values)? {
            then_value
        } else {
            else_value
        };
        return evaluate_string(branch, values);
    }
    evaluate_string(template, values)
}

fn split_conditional(value: &str) -> Option<(&str, &str, &str)> {
    let mut quote = None;
    let mut depth = 0usize;
    let mut if_start = None;
    let mut else_start = None;
    for (index, ch) in value.char_indices() {
        match (ch, quote) {
            ('"' | '\'', None) => quote = Some(ch),
            (ch, Some(current)) if ch == current => quote = None,
            ('{', None) => depth += 1,
            ('}', None) => depth = depth.saturating_sub(1),
            _ => {}
        }
        if quote.is_none() && depth == 0 {
            if value[index..].starts_with(" if ") && if_start.is_none() {
                if_start = Some(index);
            } else if value[index..].starts_with(" else ") && else_start.is_none() {
                else_start = Some(index);
            }
        }
    }
    let (Some(if_start), Some(else_start)) = (if_start, else_start) else {
        return None;
    };
    Some((
        value[..if_start].trim(),
        value[if_start + 4..else_start].trim(),
        value[else_start + 6..].trim(),
    ))
}

fn evaluate_condition(value: &str, values: &[(&str, TemplateValue)]) -> Result<bool, String> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("not ") {
        return Ok(!evaluate_condition(rest, values)?);
    }
    if let Some((left, right)) = value.split_once(" or ") {
        return Ok(evaluate_condition(left, values)? || evaluate_condition(right, values)?);
    }
    if let Some((left, right)) = value.split_once(" and ") {
        return Ok(evaluate_condition(left, values)? && evaluate_condition(right, values)?);
    }
    match lookup_template_value(value, values)? {
        TemplateValue::Bool(value) => Ok(value),
        TemplateValue::String(value) => Ok(!value.is_empty()),
    }
}

fn evaluate_string(value: &str, values: &[(&str, TemplateValue)]) -> Result<String, String> {
    let value = value.trim();
    if let Some(body) = value
        .strip_prefix("f\"")
        .and_then(|body| body.strip_suffix('"'))
    {
        return interpolate_fstring(body, values);
    }
    if let Some(body) = value
        .strip_prefix("f'")
        .and_then(|body| body.strip_suffix('\''))
    {
        return interpolate_fstring(body, values);
    }
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return Ok(value[1..value.len() - 1].to_string());
    }
    match lookup_template_value(value, values)? {
        TemplateValue::String(value) => Ok(value),
        TemplateValue::Bool(value) => Ok(value.to_string()),
    }
}

fn interpolate_fstring(body: &str, values: &[(&str, TemplateValue)]) -> Result<String, String> {
    let mut result = String::new();
    let mut remaining = body;
    while let Some(start) = remaining.find('{') {
        result.push_str(&remaining[..start]);
        let end = remaining[start + 1..]
            .find('}')
            .ok_or_else(|| "unterminated interpolation".to_string())?
            + start
            + 1;
        result.push_str(&evaluate_string(&remaining[start + 1..end], values)?);
        remaining = &remaining[end + 1..];
    }
    result.push_str(remaining);
    Ok(result)
}

fn lookup_template_value(
    value: &str,
    values: &[(&str, TemplateValue)],
) -> Result<TemplateValue, String> {
    let value = value.trim();
    let (base, mut methods) = value
        .split_once('.')
        .map_or((value, ""), |(base, methods)| (base, methods));
    let mut result = values
        .iter()
        .find(|(name, _)| *name == base)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| format!("unsupported expression '{value}'"))?;
    while !methods.is_empty() {
        let (method, rest) = methods
            .split_once('.')
            .map_or((methods, ""), |(method, rest)| (method, rest));
        if method == "upper()" {
            result = TemplateValue::String(as_string(&result).to_uppercase());
        } else if method == "lower()" {
            result = TemplateValue::String(as_string(&result).to_lowercase());
        } else if let Some(args) = method
            .strip_prefix("replace(")
            .and_then(|v| v.strip_suffix(')'))
        {
            let (from, to) = args
                .split_once(',')
                .ok_or_else(|| "replace expects two arguments".to_string())?;
            let from = unquote_template_arg(from.trim())?;
            let to = unquote_template_arg(to.trim())?;
            result = TemplateValue::String(as_string(&result).replace(&from, &to));
        } else {
            return Err(format!("unsupported method '{method}'"));
        }
        methods = rest;
    }
    Ok(result)
}

fn as_string(value: &TemplateValue) -> String {
    match value {
        TemplateValue::String(value) => value.clone(),
        TemplateValue::Bool(value) => value.to_string(),
    }
}

fn unquote_template_arg(value: &str) -> Result<String, String> {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        Ok(value[1..value.len() - 1].to_string())
    } else {
        Err(format!("expected a quoted string argument, got '{value}'"))
    }
}

fn lower_annotations(annotations: &[cst::Annotation]) -> Vec<HirAnnotation> {
    annotations
        .iter()
        .map(|annotation| HirAnnotation {
            name: annotation.name.clone(),
            args: annotation
                .args
                .iter()
                .map(|arg| HirAnnotationArg {
                    text: arg.text.clone(),
                    span: Some(arg.span.into()),
                })
                .collect(),
            span: Some(annotation.span.into()),
        })
        .collect()
}

/// Map a parsed CST settings block onto the protocol settings tree (#86).
fn lower_settings(settings: &cst::Settings) -> HirSettings {
    HirSettings {
        span: Some(settings.span.into()),
        children: settings.children.iter().map(lower_settings_node).collect(),
    }
}

fn lower_settings_node(node: &cst::SettingsNode) -> HirSettingsNode {
    match node {
        cst::SettingsNode::Group {
            name,
            children,
            span,
        } => HirSettingsNode::Group {
            name: name.clone(),
            children: children.iter().map(lower_settings_node).collect(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::Number { name, value, span } => HirSettingsNode::Number {
            name: name.clone(),
            value: *value,
            span: Some((*span).into()),
        },
        cst::SettingsNode::Bool { name, value, span } => HirSettingsNode::Bool {
            name: name.clone(),
            value: *value,
            span: Some((*span).into()),
        },
        cst::SettingsNode::String { name, value, span } => HirSettingsNode::String {
            name: name.clone(),
            value: value.clone(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::List {
            name,
            elements,
            span,
        } => HirSettingsNode::List {
            name: name.clone(),
            elements: elements
                .iter()
                .map(|element| crate::hir::types::SettingsListElement {
                    value: element.value.clone(),
                    span: Some(element.span.into()),
                })
                .collect(),
            span: Some((*span).into()),
        },
    }
}

impl Lowerer {
    fn collect_symbols(&mut self, program: &cst::Program) {
        for (order, decl) in program.declarations.iter().enumerate() {
            match decl {
                Decl::GlobalVariable { name, span, .. } => {
                    if self
                        .global_declarations
                        .insert(name.clone(), order)
                        .is_some()
                    {
                        self.error_at(
                            "duplicate-declaration",
                            format!("duplicate global variable '{name}'"),
                            *span,
                        );
                    }
                }
                Decl::PlayerVariable { name, span, .. } => {
                    if self
                        .player_declarations
                        .insert(name.clone(), order)
                        .is_some()
                    {
                        self.error_at(
                            "duplicate-declaration",
                            format!("duplicate player variable '{name}'"),
                            *span,
                        );
                    }
                }
                Decl::Subroutine { name, span, .. } => {
                    if self
                        .subroutine_declarations
                        .insert(name.clone(), order)
                        .is_some()
                    {
                        self.error_at(
                            "duplicate-declaration",
                            format!("duplicate subroutine '{name}'"),
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
        for (rule_order, entry) in program.rules.iter().enumerate() {
            let CstRuleEntry::SubroutineDef { name, span, .. } = entry else {
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
            self.subroutine_definitions
                .push((name.clone(), program.declarations.len() + rule_order));
        }
    }

    fn subroutine_visible(&self, name: &str) -> bool {
        self.subroutine_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
            || self
                .subroutine_definitions
                .iter()
                .any(|(definition, order)| definition == name && *order <= self.current_order)
    }

    fn global_visible(&self, name: &str) -> bool {
        self.global_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
    }

    fn player_visible(&self, name: &str) -> bool {
        self.player_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
    }

    fn macro_visible(&self, name: &str) -> bool {
        self.macro_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
    }

    fn enum_visible(&self, name: &str) -> bool {
        self.enum_declarations
            .get(name)
            .is_some_and(|order| *order <= self.current_order)
    }

    /// A declaration initializer: integer-`0` literal initializers are
    /// dropped (matching the reference adapter, which drops `h = 0` but
    /// carries `j = 5` and `k = 0.0`); other initializers are kept.
    fn initializer(&mut self, initializer: Option<&Expr>) -> Option<Box<HirExpr>> {
        let initializer = initializer?;
        let lowered = self.lower_expr(initializer, &[], CallPosition::Value);
        match &lowered {
            HirExpr::Number { text, .. } if text == "0" => None,
            other => Some(Box::new(other.clone())),
        }
    }

    fn lower_rule(
        &mut self,
        rule: &cst::Rule,
        files: &[SourceFile],
        preprocessing: &PreprocessingState,
    ) -> OpyResult<Rule> {
        let conditions = rule
            .conditions
            .iter()
            .map(|condition| self.lower_expr(condition, &[], CallPosition::Value))
            .collect();
        let actions = self.lower_block(&rule.actions, &[], false, true, false);
        Ok(Rule {
            name: render_rule_name(
                &rule.name,
                rule.rule_prefix.as_deref(),
                rule.delimiter,
                rule.span,
                files,
                preprocessing,
            )?,
            span: Some(rule.span.into()),
            name_span: Some(rule.name_span.into()),
            disabled: rule.disabled,
            delimiter: rule.delimiter,
            new_page: rule.new_page.clone(),
            annotations: lower_annotations(&rule.annotations),
            event: Event {
                name: rule.event.name.clone(),
                args: rule
                    .event
                    .args
                    .iter()
                    .map(|arg| self.lower_expr(arg, &[], CallPosition::Value))
                    .collect(),
                span: Some(rule.event.span.into()),
            },
            conditions,
            actions,
        })
    }

    /// Lower a statement block; `macro_params` names resolve to `MacroParam`.
    fn lower_block(
        &mut self,
        stmts: &[Stmt],
        macro_params: &[String],
        breakable: bool,
        allow_do_while: bool,
        loopable: bool,
    ) -> Vec<HirStmt> {
        stmts
            .iter()
            .enumerate()
            .map(|(index, stmt)| {
                if matches!(stmt, Stmt::DoWhile { .. })
                    && (!allow_do_while
                        || stmts[..index]
                            .iter()
                            .any(|previous| !matches!(previous, Stmt::Pass { .. })))
                {
                    self.error_at(
                        "do-while-placement",
                        "do-while must be at the beginning of a rule, subroutine, or do-while body; only pass statements may precede it".to_string(),
                        stmt.span(),
                    );
                }
                self.lower_stmt(stmt, macro_params, breakable, loopable)
            })
            .collect()
    }

    fn lower_stmt(
        &mut self,
        stmt: &Stmt,
        macro_params: &[String],
        breakable: bool,
        loopable: bool,
    ) -> HirStmt {
        match stmt {
            Stmt::Expr { expr, span } => {
                // A bare call of a declared subroutine becomes
                // `CallSubroutine` (reference behavior).
                if let Expr::Call { name, args, .. } = expr {
                    if self.subroutine_visible(name) && args.is_empty() {
                        return HirStmt::CallSubroutine {
                            name: name.clone(),
                            span: Some(span.into()),
                        };
                    }
                }
                // Statement-position builtin resolution (action/value
                // identity, unknown names) happens inside `lower_expr`.
                HirStmt::Expr {
                    expr: Box::new(self.lower_expr(expr, macro_params, CallPosition::Statement)),
                    span: Some(span.into()),
                }
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => HirStmt::Assign {
                target: Box::new(self.lower_expr(target, macro_params, CallPosition::Value)),
                value: Box::new(self.lower_expr(value, macro_params, CallPosition::Value)),
                span: Some(span.into()),
            },
            Stmt::If {
                branches,
                r#else,
                span,
            } => HirStmt::If {
                branches: branches
                    .iter()
                    .map(|branch| IfBranch {
                        condition: Box::new(self.lower_expr(
                            &branch.condition,
                            macro_params,
                            CallPosition::Value,
                        )),
                        body: self.lower_block(
                            &branch.body,
                            macro_params,
                            breakable,
                            false,
                            loopable,
                        ),
                    })
                    .collect(),
                r#else: r#else
                    .as_ref()
                    .map(|body| self.lower_block(body, macro_params, breakable, false, loopable)),
                span: Some(span.into()),
            },
            Stmt::For {
                variable,
                iterable,
                body,
                span,
            } => {
                // The reference accepts only `range(...)` as a `for ... in`
                // iterable; other iterables are an explicit frontend error
                // (recovered by lowering in value position).
                let iterable_position = if matches!(iterable, Expr::Call { name, .. } if name == "range")
                {
                    CallPosition::ForIterable
                } else {
                    self.error_at(
                        "invalid-iterable",
                        "for-loop iterable must be a range(...) call".to_string(),
                        iterable.span(),
                    );
                    CallPosition::Value
                };
                HirStmt::For {
                    variable: Box::new(self.lower_for_binder(variable, macro_params)),
                    iterable: Box::new(self.lower_expr(iterable, macro_params, iterable_position)),
                    body: self.lower_block(body, macro_params, true, false, true),
                    span: Some(span.into()),
                }
            }
            Stmt::While {
                condition,
                body,
                span,
            } => HirStmt::While {
                condition: Box::new(self.lower_expr(condition, macro_params, CallPosition::Value)),
                body: self.lower_block(body, macro_params, true, false, true),
                span: Some(span.into()),
            },
            Stmt::DoWhile {
                condition,
                body,
                span,
            } => HirStmt::DoWhile {
                condition: Box::new(self.lower_expr(condition, macro_params, CallPosition::Value)),
                body: self.lower_block(body, macro_params, true, true, true),
                span: Some(span.into()),
            },
            Stmt::Switch { value, arms, span } => HirStmt::Switch {
                value: Box::new(self.lower_expr(value, macro_params, CallPosition::Value)),
                arms: arms
                    .iter()
                    .map(|arm| match arm {
                        cst::SwitchArm::Case { value, body, span } => HirSwitchArm::Case {
                            value: Box::new(self.lower_expr(
                                value,
                                macro_params,
                                CallPosition::Value,
                            )),
                            body: self.lower_block(body, macro_params, true, false, loopable),
                            span: Some((*span).into()),
                        },
                        cst::SwitchArm::Default { body, span } => HirSwitchArm::Default {
                            body: self.lower_block(body, macro_params, true, false, loopable),
                            span: Some((*span).into()),
                        },
                    })
                    .collect(),
                span: Some(span.into()),
            },
            Stmt::Delete { target, span } => HirStmt::Delete {
                target: Box::new(self.lower_expr(target, macro_params, CallPosition::Value)),
                span: Some(span.into()),
            },
            Stmt::Break { span } => {
                if !breakable {
                    self.error_at(
                        "break-context",
                        "break is only valid inside a switch or loop".to_string(),
                        *span,
                    );
                }
                HirStmt::Break {
                    span: Some(span.into()),
                }
            }
            Stmt::Continue { span } => {
                if !loopable {
                    self.error_at(
                        "continue-context",
                        "continue is only valid inside a loop".to_string(),
                        *span,
                    );
                }
                HirStmt::Continue {
                    span: Some(span.into()),
                }
            }
            Stmt::Goto {
                label,
                offset,
                rule_start,
                span,
            } => HirStmt::Goto {
                label: label.clone(),
                offset: offset.as_ref().map(|offset| {
                    Box::new(self.lower_expr(offset, macro_params, CallPosition::Value))
                }),
                rule_start: *rule_start,
                span: Some(span.into()),
            },
            Stmt::Label { name, span } => HirStmt::Label {
                name: name.clone(),
                span: Some(span.into()),
            },
            Stmt::Pass { span } => HirStmt::Pass {
                span: Some(span.into()),
            },
        }
    }

    fn lower_for_binder(&mut self, variable: &Expr, macro_params: &[String]) -> HirExpr {
        if let Expr::Member {
            receiver,
            member,
            member_span,
            span,
        } = variable
        {
            if !default_var_index(member).is_some() && !self.player_visible(member) {
                self.error_at(
                    "unknown-identifier",
                    format!("unknown player variable '{member}'"),
                    *member_span,
                );
                return HirExpr::Null { span: None };
            }
            return HirExpr::PlayerVar {
                player: Box::new(self.lower_expr(receiver, macro_params, CallPosition::Value)),
                name: member.clone(),
                member_span: Some((*member_span).into()),
                span: Some((*span).into()),
            };
        }
        self.lower_expr(variable, macro_params, CallPosition::Value)
    }

    fn lower_macro_body(&mut self, body: &[Stmt], params: &[String]) -> Vec<HirStmt> {
        self.lower_block(body, params, false, false, false)
    }

    fn lower_expr(
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

    fn lower_name(&mut self, name: &str, span: Span, macro_params: &[String]) -> HirExpr {
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
        match name {
            "eventPlayer" => HirExpr::EventPlayer {
                span: Some(span.into()),
            },
            "hostPlayer" => HirExpr::HostPlayer {
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

    fn lower_member(
        &mut self,
        receiver: &Expr,
        member: &str,
        member_span: Span,
        span: Span,
        _macro_params: &[String],
    ) -> HirExpr {
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
            // Builtin Workshop enum: the domain name is a declared OPY
            // signature identity (manifest `param.domain`); the member list
            // is Workshop-owned catalog content, so the member access
            // resolves as an opaque identity after validating the member
            // against the canonical Workshop catalog.
            if self.manifest.domain_identity(name) {
                let locale = Locale::new("en-US");
                let catalog_member = match (name.as_str(), member) {
                    ("SpecVisibility", "NEVER") => "VISIBLE_NEVER",
                    _ => member,
                };
                let canonical_member = self
                    .catalog
                    .enum_domain(name)
                    .and_then(|domain| {
                        domain
                            .members
                            .iter()
                            .find(|candidate| candidate.member == catalog_member)
                            .map(|candidate| candidate.member.clone())
                    })
                    .or_else(|| {
                        if name == "Team" && member.parse::<u32>().is_ok() {
                            self.catalog
                                .resolve_enum_member(name, &locale, &format!("{name} {member}"))
                                .map(|(_, member)| member)
                        } else {
                            None
                        }
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
                    value_type: name.clone(),
                    value: canonical_member,
                    span: Some(span.into()),
                };
            }
            // Event-player member: a player-variable reference.
            if name == "eventPlayer" {
                if !default_var_index(member).is_some() && !self.player_visible(member) {
                    self.error_at(
                        "unknown-identifier",
                        format!("unknown player variable '{member}'"),
                        member_span,
                    );
                    return HirExpr::Null { span: None };
                }
                return HirExpr::PlayerVar {
                    player: Box::new(HirExpr::EventPlayer { span: None }),
                    name: member.to_string(),
                    member_span: Some(member_span.into()),
                    span: Some(span.into()),
                };
            }
            if name == "hostPlayer" {
                if !default_var_index(member).is_some() && !self.player_visible(member) {
                    self.error_at(
                        "unknown-identifier",
                        format!("unknown player variable '{member}'"),
                        member_span,
                    );
                    return HirExpr::Null { span: None };
                }
                return HirExpr::PlayerVar {
                    player: Box::new(HirExpr::HostPlayer { span: None }),
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
        self.error_at(
            "unsupported-member",
            "unsupported member access on this expression".to_string(),
            span,
        );
        HirExpr::Null { span: None }
    }

    fn lower_call(
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

    fn lower_workshop_setting(
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

    /// Lower call arguments to HIR values in source order (used for macro
    /// calls and unresolved names; keyword values lose their name).
    fn lower_arg_values(&mut self, args: &[cst::CallArg], macro_params: &[String]) -> Vec<HirExpr> {
        self.lower_arg_values_with_lambda(args, macro_params, |_, _| false)
    }

    fn lower_arg_values_with_lambda(
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
    fn bind_args(
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
                    Some(ParamDefault::EnumMember(member)) => {
                        let domain = param.domain.clone().unwrap_or_default();
                        bound.push(HirExpr::Enum {
                            value_type: domain,
                            value: member.clone(),
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
    fn lower_call_arg_value(
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
    /// its declared name and arguments without a domain diagnostic.
    fn resolve_contextual_domain(
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

    fn lower_receiver_call(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[cst::CallArg],
        span: Span,
        macro_params: &[String],
        position: CallPosition,
    ) -> HirExpr {
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
            if root == "eventPlayer" {
                return HirExpr::ReceiverCall {
                    receiver: Box::new(HirExpr::EventPlayer { span: None }),
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
    fn check_call_position(
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
        }
    }

    /// Check a member call's receiver against its declared category. Only
    /// the reference-enforced categories reject: `.append` requires an
    /// assignable receiver and `.format` a string literal; player-oriented
    /// members accept any receiver (the pinned reference does not type-check
    /// them).
    fn check_receiver(
        &mut self,
        receiver: &Expr,
        category: ReceiverCategory,
        entry: &Function,
        span: Span,
    ) {
        let mismatch = match category {
            ReceiverCategory::String => !matches!(receiver, Expr::String { .. }),
            ReceiverCategory::Variable => !assignable_receiver(receiver),
            ReceiverCategory::Player | ReceiverCategory::Any => false,
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
    fn check_arity(&mut self, entry: &Function, got: usize, span: Span) {
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

    fn error_at(&mut self, code: &str, message: String, span: Span) {
        self.errors.push(OpyError::at(code, message, span));
    }
}

/// The keyword spellings a parameter accepts (its name plus alternates).
fn keyword_spellings(param: &Param) -> Vec<String> {
    let mut spellings = vec![param.name.clone()];
    spellings.extend(param.alternate_names.iter().cloned());
    spellings
}

/// The source span covering a call's argument list (the start of the first
/// argument through the last argument).
fn arg_span(args: &[CallArg]) -> Span {
    args.first().map(CallArg::span).unwrap_or_else(|| {
        Span::new(
            0,
            crate::diag::Position::new(1, 1),
            crate::diag::Position::new(1, 1),
        )
    })
}

/// Whether a CST receiver is assignable (the `.append` receiver rule): a
/// variable name (including macro parameters), an array literal, or an index
/// expression — matching the pinned reference, which rejects constant and
/// function receivers ("Cannot modify or assign to …").
fn assignable_receiver(receiver: &Expr) -> bool {
    match receiver {
        Expr::Name { name, .. } => name != "eventPlayer",
        Expr::Array { .. } | Expr::Index { .. } => true,
        _ => false,
    }
}

impl From<Span> for HirSpan {
    fn from(span: Span) -> HirSpan {
        HirSpan {
            file: span.file,
            start: Position {
                line: span.start.line,
                col: span.start.col,
            },
            end: Position {
                line: span.end.line,
                col: span.end.col,
            },
        }
    }
}

impl From<&Span> for HirSpan {
    fn from(span: &Span) -> HirSpan {
        (*span).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::types::{Expr as HirExpr, RuleEntry as HirRuleEntry, Stmt as HirStmt};
    use crate::lexer::{LexInput, lex};
    use crate::parser::parse;

    fn lower_ok(text: &str) -> HirProgram {
        let tokens = lex(LexInput { file_id: 0, text }).expect("lexes");
        let output = parse(&tokens);
        assert!(
            output.errors.is_empty(),
            "unexpected parse errors: {:?}",
            output.errors
        );
        let program = output.program.expect("parse produces a program");
        lower(&program, vec![], vec![]).expect("lowers without errors")
    }

    fn rule_conditions_and_actions(hir: &HirProgram) -> (&Vec<HirExpr>, &Vec<HirStmt>) {
        let HirRuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        (&rule.conditions, &rule.actions)
    }

    #[test]
    fn producer_emits_the_v2_ordered_switch_contract() {
        let hir = lower_ok(
            "globalvar value\nrule \"r\":\n    @Event global\n    switch value:\n        default:\n            value = 1\n        case 2:\n            value = 2\n",
        );
        assert_eq!(hir.protocol.name, "wright/opy-hir");
        assert_eq!(hir.protocol.version, "2.0.0");
        let value = serde_json::to_value(&hir).expect("HIR must serialize");
        let switch = &value["rules"][0]["actions"][0];
        assert!(switch.get("arms").is_some());
        assert!(switch.get("cases").is_none());
        assert!(switch.get("default").is_none());
    }

    #[test]
    fn receiver_calls_lower_to_receiver_call_hir() {
        // `eventPlayer.setMoveSpeed(100)` lowers to a ReceiverCall on the
        // event player, and `target.setMoveSpeed(50)` to a ReceiverCall on a
        // global-variable receiver (#104).
        let hir = lower_ok(
            "globalvar target\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.setMoveSpeed(100)\n    target.setMoveSpeed(50)\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        assert_eq!(actions.len(), 2);

        let HirStmt::Expr { expr, .. } = &actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } = expr.as_ref()
        else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "setMoveSpeed");
        assert!(matches!(receiver.as_ref(), HirExpr::EventPlayer { .. }));
        assert_eq!(args.len(), 1);
        assert!(matches!(&args[0], HirExpr::Number { .. }));

        let HirStmt::Expr { expr, .. } = &actions[1] else {
            panic!("expected expression statement");
        };
        let HirExpr::ReceiverCall { receiver, name, .. } = expr.as_ref() else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "setMoveSpeed");
        assert!(
            matches!(receiver.as_ref(), HirExpr::GlobalVar { name, .. } if name == "target"),
            "globalvar receiver must resolve to a GlobalVar"
        );
    }

    #[test]
    fn bare_variable_member_expression_preserves_receiver_and_member() {
        let hir = lower_ok(
            "globalvar A\nplayervar B\nrule \"receiver\":\n    @Event eachPlayer\n    A = B.C\n",
        );
        let HirStmt::Assign { value, .. } = &hir
            .rules
            .iter()
            .find_map(|entry| {
                let RuleEntry::Rule(rule) = entry else {
                    return None;
                };
                rule.actions.first()
            })
            .expect("assignment")
        else {
            panic!("expected assignment");
        };
        let HirExpr::Member {
            receiver, member, ..
        } = value.as_ref()
        else {
            panic!("expected opaque member expression, got {value:?}");
        };
        assert_eq!(member, "C");
        assert!(matches!(receiver.as_ref(), HirExpr::GlobalVar { name, .. } if name == "B"));
    }

    #[test]
    fn rule_prefix_template_is_global_and_subroutine_identity_is_preserved() {
        let text = "rule \"before\":\n    pass\ndef source_name():\n    @Name \"Friendly\"\n    pass\nrule \"after\":\n    pass\n";
        let tokens = lex(LexInput { file_id: 0, text }).expect("lexes");
        let output = parse(&tokens);
        assert!(
            output.errors.is_empty(),
            "unexpected parse errors: {:?}",
            output.errors
        );
        let program = output.program.expect("program");
        let preprocessing = PreprocessingState {
            rule_prefix_template: Some(crate::hir::types::DirectiveValue {
                value: "f\"[{$pathTitle.replace('_', ' ')}] {$rule}\" if $rule and not $isDelimiter else $rule".to_string(),
                span: None,
            }),
            ..PreprocessingState::default()
        };
        let hir = lower_with_preprocessing(
            &program,
            vec![SourceFile {
                id: 0,
                path: "main.opy".to_string(),
            }],
            vec![],
            &preprocessing,
        )
        .expect("lowers");
        let names: Vec<_> = hir
            .rules
            .iter()
            .map(|entry| match entry {
                HirRuleEntry::Rule(rule) => rule.name.clone(),
                HirRuleEntry::SubroutineDef { name, .. } => name.clone(),
            })
            .collect();
        assert_eq!(
            names,
            vec!["[Main] before", "[Main] Friendly", "[Main] after"]
        );
        let HirRuleEntry::SubroutineDef {
            name, source_name, ..
        } = &hir.rules[1]
        else {
            panic!("expected subroutine definition");
        };
        assert_eq!(name, "[Main] Friendly");
        assert_eq!(source_name, "source_name");
    }

    #[test]
    fn receiver_call_values_lower_in_conditions() {
        // `@Condition eventPlayer.isAlive()` lowers to a ReceiverCall value;
        // `eventPlayer.teleport(eventPlayer.getPosition())` nests a receiver
        // call inside another receiver call's arguments (#104).
        let hir = lower_ok(
            "rule \"r\":\n    @Event eachPlayer\n    @Condition eventPlayer.isAlive()\n    eventPlayer.teleport(eventPlayer.getPosition())\n",
        );
        let (conditions, actions) = rule_conditions_and_actions(&hir);
        assert_eq!(conditions.len(), 1);
        let HirExpr::ReceiverCall { name, args, .. } = &conditions[0] else {
            panic!("expected receiver call condition, got {:?}", conditions[0]);
        };
        assert_eq!(name, "isAlive");
        assert_eq!(args.len(), 0);

        let HirStmt::Expr { expr, .. } = &actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::ReceiverCall {
            name,
            args,
            receiver,
            ..
        } = expr.as_ref()
        else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "teleport");
        assert!(matches!(receiver.as_ref(), HirExpr::EventPlayer { .. }));
        assert_eq!(args.len(), 1);
        assert!(matches!(
            &args[0],
            HirExpr::ReceiverCall { name, .. } if name == "getPosition"
        ));
    }

    #[test]
    fn format_string_receiver_stays_a_format_node() {
        // `.format()` on a string receiver is unaffected by the receiver-call
        // path (existing supported form).
        let hir = lower_ok(
            "rule \"r\":\n    @Event global\n    print(\"{} points\".format(len([1, 2])))\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::Expr { expr, .. } = &actions[0] else {
            panic!("expected expression statement");
        };
        assert!(
            has_format(expr),
            "string `.format()` must lower to a Format node"
        );
    }

    fn has_format(expr: &HirExpr) -> bool {
        match expr {
            HirExpr::Format { .. } => true,
            HirExpr::Call { args, .. } => args.iter().any(has_format),
            HirExpr::ReceiverCall { args, .. } => args.iter().any(has_format),
            _ => false,
        }
    }

    /// Lower one rule action and return the assignment's value expression.
    fn lowered_value(source: &str) -> HirExpr {
        let program = crate::compile(source, "test.opy", std::path::Path::new(""))
            .unwrap_or_else(|error| panic!("compile failed: {error}"));
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Assign { value, .. } = &rule.actions[0] else {
            panic!("expected an assign statement");
        };
        (**value).clone()
    }

    #[test]
    fn chase_time_reeval_none_lowers_to_the_catalog_enum() {
        let value = lowered_value(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseTimeReeval.NONE\n",
        );
        assert_enum(&value, "ChaseTimeReeval", "NONE");
    }

    #[test]
    fn chase_time_reeval_destination_and_duration_lowers_to_the_catalog_enum() {
        let value = lowered_value(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseTimeReeval.DESTINATION_AND_DURATION\n",
        );
        assert_enum(&value, "ChaseTimeReeval", "DESTINATION_AND_DURATION");
    }

    #[test]
    fn chase_rate_reeval_members_lower_to_the_catalog_enum() {
        for member in ["NONE", "DESTINATION_AND_RATE"] {
            let source = format!(
                "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseRateReeval.{member}\n"
            );
            assert_enum(&lowered_value(&source), "ChaseRateReeval", member);
        }
    }

    /// Assert the expression is the catalog enum `(domain, member)` node,
    /// ignoring its source span (the span is frontend-internal provenance).
    fn assert_enum(value: &HirExpr, domain: &str, member: &str) {
        match value {
            HirExpr::Enum {
                value_type, value, ..
            } => {
                assert_eq!(value_type, domain);
                assert_eq!(value, member);
            }
            other => panic!("expected enum {domain}.{member}, got {other:?}"),
        }
    }

    #[test]
    fn unknown_chase_time_reeval_member_is_rejected_by_the_catalog() {
        let error = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseTimeReeval.NOPE\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect_err("unknown catalog member must be rejected");
        assert_eq!(error.code, "unknown-enum-member");
    }

    #[test]
    fn unknown_enum_receiver_is_an_unsupported_member_error() {
        let error = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = NotARealEnum.MEMBER\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect_err("an unknown enum type must fail");
        assert_eq!(error.code, "unsupported-member");
        let span = error.span.expect("the error is source-located");
        assert_eq!(span.start.line, 4);
    }

    // --- Builtin semantic manifest coverage (#109) ---

    /// Assert a compile failure has the given code at the given line.
    fn compile_error(source: &str, line: u32) -> OpyError {
        let error = crate::compile(source, "test.opy", std::path::Path::new(""))
            .expect_err("expected a compile failure");
        let span = error.span.expect("the error is source-located");
        assert_eq!(span.start.line, line, "code '{}'", error.code);
        error
    }

    fn action_source(statement: &str) -> String {
        format!("globalvar g\nrule \"r\":\n    @Event global\n    {statement}\n")
    }

    #[test]
    fn chase_over_time_resolves_and_compiles_with_reference_signatures() {
        // 4-argument form with an explicit reevaluation member (#106).
        let hir = crate::compile(
            &action_source("chaseOverTime(g, 10, 3, ChaseTimeReeval.NONE)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("reference-supported chaseOverTime compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::Call { name, args, .. } = expr.as_ref() else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseOverTime");
        assert_eq!(args.len(), 4);
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseTimeReeval" && value == "NONE"
        ));

        // 3-argument form fills the reference default member.
        let hir = crate::compile(
            &action_source("chaseOverTime(g, 10, 3)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("default-reevaluation chaseOverTime compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::Call { args, .. } = expr.as_ref() else {
            panic!("expected a call");
        };
        assert_eq!(args.len(), 4);
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseTimeReeval" && value == "DESTINATION_AND_DURATION"
        ));
    }

    #[test]
    fn is_game_in_progress_resolves_as_a_builtin_value() {
        // Generic value gap from #106: `isGameInProgress()` in a condition.
        let hir = crate::compile(
            &action_source("@Condition isGameInProgress() == true"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("reference-supported isGameInProgress compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        assert!(matches!(&rule.conditions[0], HirExpr::Binary { .. }));
    }

    #[test]
    fn enum_gated_members_resolve_through_the_manifest() {
        // Enum-gated members from #106: setInvisibility (Invis), getThrottle
        // (member value), worldVector (Transform arg), setStatusEffect
        // (Status arg).
        let source = "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
            @Condition eventPlayer.getThrottle() != vect(0, 0, 0)\n    \
            @Condition worldVector(vect(1, 2, 3), eventPlayer, Transform.ROTATION) != vect(0, 0, 0)\n    \
            eventPlayer.setInvisibility(Invis.ALL)\n    \
            eventPlayer.setStatusEffect(eventPlayer, Status.ROOTED, 2)\n";
        let hir = crate::compile(source, "test.opy", std::path::Path::new(""))
            .expect("enum-gated members compile");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        assert_eq!(rule.actions.len(), 2);
    }

    #[test]
    fn get_players_in_radius_fills_reference_enum_defaults() {
        // 2-argument form fills Team.ALL and LosCheck.OFF (reference
        // emission: `Players Within Radius(..., All Teams, Off)`).
        let hir = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
             @Condition len(getPlayersInRadius(eventPlayer.getPosition(), 10)) > 0\n    \
             disableInspector()\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("getPlayersInRadius with defaults compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirExpr::Binary { left, .. } = &rule.conditions[0] else {
            panic!("expected a comparison");
        };
        let HirExpr::Call { name, args, .. } = left.as_ref() else {
            panic!("expected len call");
        };
        assert_eq!(name, "len");
        let HirExpr::Call { name, args, .. } = &args[0] else {
            panic!("expected getPlayersInRadius call");
        };
        assert_eq!(name, "getPlayersInRadius");
        assert_eq!(args.len(), 4);
        assert!(matches!(
            &args[2],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "Team" && value == "ALL"
        ));
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "LosCheck" && value == "OFF"
        ));
    }

    #[test]
    fn value_call_in_action_position_is_rejected() {
        let error = compile_error(&action_source("isGameInProgress()"), 4);
        assert_eq!(error.code, "value-in-action-position");
    }

    #[test]
    fn value_member_in_action_position_is_rejected() {
        // The #106 baseline records the oracle rejecting `B.isAlive()` as a
        // statement; the manifest enforces that contract (#109).
        let error = compile_error(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.isAlive()\n",
            4,
        );
        assert_eq!(error.code, "value-in-action-position");
    }

    // --- Named/keyword argument binding and chase call context (#110) ---

    /// Compile a program and return the lowered first action's expression
    /// (the statement expression, or the value of a leading assignment).
    fn first_action_expr(source: &str) -> HirExpr {
        let program = crate::compile(source, "test.opy", std::path::Path::new(""))
            .unwrap_or_else(|error| panic!("compile failed: {error}"));
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        match &rule.actions[0] {
            HirStmt::Expr { expr, .. } => (**expr).clone(),
            HirStmt::Assign { value, .. } => (**value).clone(),
            other => panic!("expected an expression or assignment, got {other:?}"),
        }
    }

    /// Remove every `span`/`name_span` key from a serialized expression (the
    /// differential suite's normalization).
    fn strip_spans(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("span");
                map.remove("name_span");
                for nested in map.values_mut() {
                    strip_spans(nested);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    strip_spans(item);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn chase_keyword_forms_dispatch_to_the_concrete_chase_functions() {
        // The reference `chase` form: `rate = …` dispatches to chaseAtRate
        // with the ChaseRateReeval domain, `duration = …` to chaseOverTime
        // with the ChaseTimeReeval domain; the `ChaseReeval` member resolves
        // only through this call context (issue #110).
        let expr = first_action_expr(&action_source("chase(g, 10, rate=2, ChaseReeval.NONE)"));
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseAtRate");
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseRateReeval" && value == "NONE"
        ));

        let expr = first_action_expr(&action_source(
            "chase(g, 10, duration=3, ChaseReeval.DESTINATION_AND_DURATION)",
        ));
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseOverTime");
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseTimeReeval" && value == "DESTINATION_AND_DURATION"
        ));

        // Player-variable first arguments resolve too (the emission layer
        // picks the player form).
        let expr = first_action_expr(
            "playervar P\nrule \"r\":\n    @Event eachPlayer\n    \
             chase(eventPlayer.P, 0, rate=1, ChaseReeval.NONE)\n",
        );
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseAtRate");
        assert!(matches!(&args[0], HirExpr::PlayerVar { .. }));
    }

    #[test]
    fn chase_reeval_is_only_a_standalone_identity_inside_the_chase_context() {
        // `ChaseReeval` is a contextual domain, not a standalone domain
        // identity: a bare member access outside the chase signature is
        // rejected.
        let error = compile_error(&action_source("g = ChaseReeval.NONE"), 4);
        assert_eq!(error.code, "unsupported-member");

        // Inside the chase context the member is an opaque identity
        // dispatched to the concrete domain selected by the keyword
        // selector; member existence in that domain is not validated here
        // (lowering-dependent, #8).
        let expr = first_action_expr(&action_source(
            "chase(g, 10, rate=2, ChaseReeval.DESTINATION_AND_DURATION)",
        ));
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseAtRate");
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseRateReeval" && value == "DESTINATION_AND_DURATION"
        ));

        // A non-enum 4th argument is carried structurally; the reference's
        // "expected an enum member" rejection is lowering-dependent.
        let expr = first_action_expr(&action_source("chase(g, 10, rate=2, 5)"));
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chase");
        assert!(matches!(&args[3], HirExpr::Number { .. }));
    }

    #[test]
    fn chase_requires_the_keyword_rate_or_duration_third_argument() {
        let error = compile_error(&action_source("chase(g, 10, 2, ChaseReeval.NONE)"), 4);
        assert_eq!(error.code, "keyword-required");
        assert!(error.message.contains("rate"));
    }

    #[test]
    fn chase_family_requires_a_variable_first_argument() {
        // The reference rejects non-variable first arguments for the chase
        // family ("Expected variable for 1st argument of function
        // 'chaseOverTime'", issue #110) — the variable kind also selects
        // the global/player emission form.
        let error = compile_error(&action_source("chase(10, 10, rate=2, ChaseReeval.NONE)"), 4);
        assert_eq!(error.code, "invalid-argument");

        let error = compile_error(
            &action_source("chaseOverTime(10, 0, 30, ChaseTimeReeval.NONE)"),
            4,
        );
        assert_eq!(error.code, "invalid-argument");
    }

    #[test]
    fn keyword_binding_matches_positional_binding_in_hir() {
        // Keyword binding consumes the manifest signatures: the bound HIR is
        // identical to the positional form's (defaults filled the same way),
        // modulo source spans (the keyword values sit at different columns).
        fn without_spans(expr: &HirExpr) -> serde_json::Value {
            let mut value = serde_json::to_value(expr).unwrap();
            strip_spans(&mut value);
            value
        }
        let keyword = without_spans(&first_action_expr(&action_source(
            "chaseOverTime(g, 10, duration=3)",
        )));
        let positional = without_spans(&first_action_expr(&action_source(
            "chaseOverTime(g, 10, 3)",
        )));
        assert_eq!(keyword, positional);

        let keyword = without_spans(&first_action_expr(&action_source("wait(time=1)")));
        let positional = without_spans(&first_action_expr(&action_source("wait(1)")));
        assert_eq!(keyword, positional);

        // Out-of-order keywords bind by name.
        let keyword = without_spans(&first_action_expr(&action_source(
            "wait(waitBehavior=Wait.IGNORE_CONDITION, time=2)",
        )));
        let positional = without_spans(&first_action_expr(&action_source("wait(2)")));
        assert_eq!(keyword, positional);

        let keyword = without_spans(&first_action_expr(&action_source(
            "g = vect(x=1, y=2, z=3)",
        )));
        let positional = without_spans(&first_action_expr(&action_source("g = vect(1, 2, 3)")));
        assert_eq!(keyword, positional);
    }

    #[test]
    fn keyword_binding_diagnostics_are_structured_and_source_located() {
        // Unknown keyword name.
        let error = compile_error(&action_source("chaseOverTime(g, 10, bogus=1)"), 4);
        assert_eq!(error.code, "unknown-keyword");
        assert!(error.message.contains("bogus"));

        // Duplicate (positional slot filled again by keyword).
        let error = compile_error(
            &action_source(
                "chaseOverTime(g, 10, 3, ChaseTimeReeval.NONE, \
                 reevaluation=ChaseTimeReeval.NONE)",
            ),
            4,
        );
        assert_eq!(error.code, "duplicate-argument");

        // Positional after keyword.
        let error = compile_error(&action_source("chaseOverTime(g, duration=3, 5)"), 4);
        assert_eq!(error.code, "positional-after-keyword");

        // Missing required argument (reference: "Missing argument 'duration'").
        let error = compile_error(&action_source("chaseOverTime(g, 10)"), 4);
        assert_eq!(error.code, "missing-argument");

        // Positional-only parameter bound by keyword (`chase`'s leading
        // arguments; the reference rejects the keyword form).
        let error = compile_error(
            &action_source("chase(variable=g, destination=10, rate=2, ChaseReeval.NONE)"),
            4,
        );
        assert_eq!(error.code, "unknown-keyword");
    }

    #[test]
    fn keyword_arguments_are_rejected_for_reference_special_cases() {
        // The reference routes `range`, `random.*`, and `.format` around its
        // generic keyword binder; keyword arguments fail deterministically.
        let error = compile_error(
            "globalvar g\nrule \"r\":\n    @Event global\n    \
             for I in range(start=0, stop=3):\n        debug(I)\n",
            4,
        );
        assert_eq!(error.code, "keyword-unsupported");

        let error = compile_error(&action_source("g = random.uniform(min=1, max=2)"), 4);
        assert_eq!(error.code, "keyword-unsupported");

        let error = compile_error(&action_source("print(\"{} points\".format(value=1))"), 4);
        assert_eq!(error.code, "keyword-unsupported");
    }

    #[test]
    fn wait_uses_the_reference_keyword_names() {
        // The manifest's `wait` parameter names match the pinned reference
        // (`time`, `waitBehavior`), so `wait(duration=1)` is an unknown
        // keyword exactly like the oracle.
        let error = compile_error(&action_source("wait(duration=1)"), 4);
        assert_eq!(error.code, "unknown-keyword");
        assert!(error.message.contains("duration"));
    }

    #[test]
    fn action_call_in_value_position_is_rejected() {
        let error = compile_error(&action_source("g = wait(1)"), 4);
        assert_eq!(error.code, "action-in-value-position");
    }

    #[test]
    fn missing_required_argument_is_a_source_located_diagnostic() {
        // Too-few calls reject with the reference's missing-argument
        // diagnostic (`chaseOverTime(g, 10)` → "Missing argument 'duration'",
        // issue #110); positional overflow keeps `invalid-arity`.
        let error = compile_error(&action_source("chaseOverTime(g, 10)"), 4);
        assert_eq!(error.code, "missing-argument");
        assert!(error.message.contains("duration"));

        let error = compile_error(&action_source("chaseOverTime(g, 10, 3, 4, 5)"), 4);
        assert_eq!(error.code, "invalid-arity");
    }

    #[test]
    fn missing_member_argument_is_a_source_located_diagnostic() {
        // #106 evidence: `getPlayersInRadius(...).setStatusEffect(eventPlayer,
        // 30)` must reject like the oracle (the `status` argument is
        // missing; the reference: "Missing argument 'status' for function
        // '.setStatusEffect'", issue #110).
        let error = compile_error(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
             getPlayersInRadius(eventPlayer.getPosition(), 10).setStatusEffect(eventPlayer, 30)\n",
            4,
        );
        assert_eq!(error.code, "missing-argument");
        assert!(error.message.contains("duration"));
    }

    #[test]
    fn invalid_receiver_categories_are_rejected() {
        // `.append` requires an assignable receiver; `.format` a string
        // literal (both reference-enforced categories).
        let error = compile_error(&action_source("3.append(1)"), 4);
        assert_eq!(error.code, "invalid-receiver");
        assert!(error.message.contains("append"));

        let error = compile_error(&action_source("print(3.format(\"{}\"))"), 4);
        assert_eq!(error.code, "invalid-receiver");
        assert!(error.message.contains("format"));
    }

    #[test]
    fn cross_domain_enum_arguments_resolve_as_opaque_identities() {
        // Domain mismatches need canonical Workshop enum knowledge: a member
        // of the wrong domain is carried as an opaque identity and the check
        // is lowering-dependent (#8).
        let expr = first_action_expr(&action_source("chaseOverTime(g, 10, 3, Invis.ALL)"));
        let HirExpr::Call { args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "Invis" && value == "ALL"
        ));

        let expr = first_action_expr(&action_source(
            "eventPlayer.setInvisibility(ChaseTimeReeval.NONE)",
        ));
        let HirExpr::ReceiverCall { args, .. } = &expr else {
            panic!("expected a receiver call, got {expr:?}");
        };
        assert!(matches!(
            &args[0],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseTimeReeval" && value == "NONE"
        ));
    }

    #[test]
    fn non_enum_arguments_for_enum_parameters_are_carried_structurally() {
        // Whether a non-enum value is acceptable for an enum parameter is
        // Workshop catalog knowledge; the frontend carries the argument
        // structurally and leaves the check to lowering (#8).
        let expr = first_action_expr(&action_source("eventPlayer.setInvisibility(g)"));
        let HirExpr::ReceiverCall { args, .. } = &expr else {
            panic!("expected a receiver call, got {expr:?}");
        };
        assert!(matches!(
            &args[0],
            HirExpr::GlobalVar { name, .. } if name == "g"
        ));

        let expr = first_action_expr(&action_source("eventPlayer.setInvisibility(3)"));
        let HirExpr::ReceiverCall { args, .. } = &expr else {
            panic!("expected a receiver call, got {expr:?}");
        };
        assert!(matches!(&args[0], HirExpr::Number { .. }));
    }

    #[test]
    fn unknown_builtins_fail_at_resolution_not_emission() {
        let error = compile_error(&action_source("frobnicate()"), 4);
        assert_eq!(error.code, "unknown-action");

        let error = compile_error(&action_source("g = frobnicate()"), 4);
        assert_eq!(error.code, "unknown-value");

        let error = compile_error(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.frobnicate()\n",
            4,
        );
        assert_eq!(error.code, "unknown-member");
    }

    #[test]
    fn wright_only_catalog_names_are_rejected() {
        // `createHudText` and `squareRoot` are Workshop emission spellings,
        // not OPY source functions; the pinned reference rejects them, so
        // the manifest does not preserve the accidental acceptance.
        let error = compile_error(&action_source("createHudText(1)"), 4);
        assert_eq!(error.code, "unknown-action");

        let error = compile_error(&action_source("g = squareRoot(9)"), 4);
        assert_eq!(error.code, "unknown-value");
    }

    #[test]
    fn generic_member_only_actions_are_rejected() {
        // `setMoveSpeed(eventPlayer, 100)` is not an OPY function: the
        // member form is the reference surface.
        let error = compile_error(&action_source("setMoveSpeed(eventPlayer, 100)"), 4);
        assert_eq!(error.code, "unknown-action");
    }

    #[test]
    fn range_is_for_iterables_only() {
        // Standalone `range(...)` is rejected by the reference; the
        // for-header form keeps 1-3 arguments.
        let error = compile_error(&action_source("@Condition len(range(1, 5, 1)) > 0"), 4);
        assert_eq!(error.code, "invalid-call-context");

        let error = compile_error(&action_source("for g in [1, 2]:\n        debug(g)"), 4);
        assert_eq!(error.code, "invalid-iterable");

        crate::compile(
            &action_source("for g in range(3):\n        debug(g)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("the for-header range form compiles");
    }

    #[test]
    fn source_aliases_resolve_to_canonical_names() {
        // Non-contextual aliases rewrite to the canonical entry so identity,
        // position, and emission use the target name.
        let hir = crate::compile(
            &action_source("stopChasingVariable(g)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("the alias target compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::Call { name, .. } = expr.as_ref() else {
            panic!("expected a call");
        };
        assert_eq!(name, "stopChasing");

        let hir = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
             @Condition eventPlayer.getCurrentHero() != null\n    \
             @Condition eventPlayer.hasStatusEffect(Status.BURNING) == false\n    \
             disableInspector()\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("member aliases compile");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirExpr::Binary { left, .. } = &rule.conditions[0] else {
            panic!("expected a comparison");
        };
        let HirExpr::ReceiverCall { name, .. } = left.as_ref() else {
            panic!("expected a receiver call");
        };
        assert_eq!(name, "getHero");
    }

    #[test]
    fn unknown_catalog_enum_members_are_rejected() {
        for source in [
            "globalvar g\nrule \"r\":\n    @Event global\n    g = Color.CYAN\n",
            "globalvar g\nrule \"r\":\n    @Event global\n    g = DynamicEffect.SPARKLES\n",
        ] {
            let error = crate::compile(source, "test.opy", std::path::Path::new(""))
                .expect_err("unknown catalog member must be rejected");
            assert_eq!(error.code, "unknown-enum-member");
        }
    }

    #[test]
    fn default_var_for_binder_resolves_at_all_range_arities() {
        // The agent-lab regression: `for I in range(0, 10):` with `I` not
        // declared. `I` is an OverPy default variable name (A–Z, AA–…), which
        // the pinned reference accepts as an implicit global loop binder
        // (#114). All range arities keep compiling (1, 2, and 3 arguments).
        for (binder, iterable) in [
            ("I", "range(0, 10)"),
            ("I", "range(3)"),
            ("I", "range(1, 5, 2)"),
        ] {
            let hir = lower_ok(&format!(
                "globalvar total\nrule \"r\":\n    @Event global\n    for {binder} in {iterable}:\n        total += {binder}\n"
            ));
            let (_, actions) = rule_conditions_and_actions(&hir);
            let HirStmt::For { variable, body, .. } = &actions[0] else {
                panic!("expected a for statement");
            };
            assert!(
                matches!(variable.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I"),
                "the binder resolves to the implicit global 'I', got {variable:?}"
            );
            assert!(!body.is_empty(), "the loop body lowers");
            // The binder use in the body resolves too: `total += I` has a
            // GlobalVar operand.
            let HirStmt::Assign { value, .. } = &body[0] else {
                panic!("expected an assignment in the body");
            };
            let HirExpr::Binary { right, .. } = value.as_ref() else {
                panic!("expected a binary expression");
            };
            assert!(
                matches!(right.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I"),
                "the binder use inside the body resolves to the implicit global"
            );
        }
    }

    #[test]
    fn player_variable_range_binder_preserves_host_player_receiver() {
        let hir = lower_ok(
            "playervar I\nrule \"r\":\n    @Event global\n    for hostPlayer.I in range(3):\n        hostPlayer.I = 1\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::For { variable, .. } = &actions[0] else {
            panic!("expected a for statement");
        };
        let HirExpr::PlayerVar {
            player,
            name,
            member_span,
            span,
        } = variable.as_ref()
        else {
            panic!("expected a player-variable binder, got {variable:?}");
        };
        assert_eq!(name, "I");
        assert!(matches!(player.as_ref(), HirExpr::HostPlayer { .. }));
        assert_eq!(span.unwrap().start.line, 4);
        assert_eq!(span.unwrap().start.col, 9);
        assert_eq!(span.unwrap().end.line, 4);
        assert_eq!(span.unwrap().end.col, 21);
        let member_span = member_span.expect("player binder member span");
        assert_eq!(member_span.start.line, 4);
        assert_eq!(member_span.start.col, 20);
        assert_eq!(member_span.end.line, 4);
        assert_eq!(member_span.end.col, 21);
    }

    #[test]
    fn default_var_names_resolve_as_implicit_globals() {
        // Default variable names resolve anywhere a variable may appear,
        // matching the pinned reference (no `globalvar` declaration needed).
        let hir = lower_ok("rule \"r\":\n    @Event global\n    I = 5\n    debug(I)\n");
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::Assign { target, .. } = &actions[0] else {
            panic!("expected an assignment");
        };
        assert!(
            matches!(target.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I"),
            "the implicit global resolves, got {target:?}"
        );
        // `AA` (slot 26) and `Z` (slot 25) are default names; `i` is not.
        assert_eq!(default_var_index("I"), Some(8));
        assert_eq!(default_var_index("AA"), Some(26));
        assert_eq!(default_var_index("Z"), Some(25));
        assert_eq!(default_var_index("DX"), Some(127));
        assert_eq!(default_var_index("DY"), None);
        assert_eq!(default_var_index("i"), None);
    }

    #[test]
    fn nested_same_name_for_binders_reuse_the_implicit_global() {
        // Nested loops with the same default-var binder reuse the single
        // implicit variable, matching the pinned reference (the inner loop
        // overwrites the same Workshop global — no separate binding).
        let hir = lower_ok(
            "rule \"r\":\n    @Event global\n    for I in range(3):\n        for I in range(2):\n            debug(I)\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::For {
            variable: outer,
            body,
            ..
        } = &actions[0]
        else {
            panic!("expected an outer for statement");
        };
        let HirStmt::For {
            variable: inner, ..
        } = &body[0]
        else {
            panic!("expected an inner for statement");
        };
        assert!(
            matches!(outer.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I")
                && matches!(inner.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I"),
            "both loops bind the same implicit global (spans differ per binder site)"
        );
    }

    #[test]
    fn undeclared_lowercase_binder_is_still_an_unknown_identifier() {
        // A lowercase undeclared binder is not a default variable name; the
        // pinned reference rejects the program ("Unknown function name"), and
        // Wright reports the same reject with the structured
        // `unknown-identifier` diagnostic (#114).
        let error = compile_error(
            "rule \"r\":\n    @Event global\n    for i in range(3):\n        debug(i)\n",
            3,
        );
        assert_eq!(error.code, "unknown-identifier");
        let span = error.span.expect("the error is source-located");
        assert_eq!(span.start.line, 3);
    }

    #[test]
    fn issue_28_constructs_lower_to_provenance_preserving_hir() {
        let hir = lower_ok(
            "globalvar x\nrule \"r\":\n    @Event global\n    do:\n        x = {\"x\": 1}[\"x\"]\n    while x not in [2, 3]\n    switch x:\n        case 0x10:\n            x = 1 in [1, 2]\n        default:\n            x = 2\n    x = [value * 2 for value, index in [1, 2] if value > index]\n    x = sorted([1, 2], key=lambda value: value)\n    x = w\"wide\"\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::DoWhile { condition, .. } = &actions[0] else {
            panic!("expected do-while");
        };
        assert!(matches!(condition.as_ref(), HirExpr::Binary { op, .. } if op == "not in"));
        let HirStmt::Switch { arms, .. } = &actions[1] else {
            panic!("expected switch");
        };
        assert_eq!(arms.len(), 2);
        let HirSwitchArm::Case {
            value: case_value,
            body,
            ..
        } = &arms[0]
        else {
            panic!("expected case arm");
        };
        assert!(
            matches!(case_value.as_ref(), HirExpr::Number { value, .. } if *value == 0x10 as f64)
        );
        let HirStmt::Assign { value, .. } = &body[0] else {
            panic!("expected case assignment");
        };
        assert!(matches!(value.as_ref(), HirExpr::Binary { op, .. } if op == "in"));
        let HirSwitchArm::Default { body, .. } = &arms[1] else {
            panic!("expected default arm");
        };
        assert!(matches!(body[0], HirStmt::Assign { .. }));
        let HirStmt::Assign { value, .. } = &actions[2] else {
            panic!("expected comprehension assignment");
        };
        assert!(matches!(value.as_ref(), HirExpr::Comprehension { .. }));
        let HirStmt::Assign { value, .. } = &actions[3] else {
            panic!("expected sorted assignment");
        };
        assert!(
            matches!(value.as_ref(), HirExpr::Call { name, args, .. } if name == "sorted" && matches!(&args[1], HirExpr::Lambda { body, .. } if matches!(body.as_ref(), HirExpr::Local { name, .. } if name == "value")))
        );
        let HirStmt::Assign { value, .. } = &actions[4] else {
            panic!("expected string assignment");
        };
        assert!(
            matches!(value.as_ref(), HirExpr::StringModifier { modifier, .. } if modifier == "w")
        );
    }

    #[test]
    fn issue_28_rejects_reference_invalid_bare_dict_and_lambda() {
        let dict_error = compile_error(
            "globalvar x\nrule \"r\":\n    @Event global\n    x = {\"x\": 1}\n",
            4,
        );
        assert_eq!(dict_error.code, "dict-access");
        let lambda_error = compile_error(
            "globalvar x\nrule \"r\":\n    @Event global\n    x = lambda value: value\n",
            4,
        );
        assert_eq!(lambda_error.code, "lambda-context");
    }

    #[test]
    fn do_while_requires_rule_or_definition_prefix_position() {
        let error = compile_error(
            "globalvar value\nrule \"r\":\n    @Event global\n    value = 1\n    do:\n        value += 1\n    while value < 2\n",
            5,
        );
        assert_eq!(error.code, "do-while-placement");
        assert_eq!(
            error.message,
            "do-while must be at the beginning of a rule, subroutine, or do-while body; only pass statements may precede it"
        );
    }
}
