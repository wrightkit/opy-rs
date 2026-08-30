//! Opy HIR v2 validation.
//!
//! Validation follows the order of the Opy HIR v2 specification (`docs/hir/
//! opy-hir-v2.md`) §8: the protocol envelope is
//! checked first (in [`super::parse_value`]), then unknown node kinds are
//! rejected with the offending kind name and span, then the payload is
//! deserialized, then structural invariants (spans, identifiers, references)
//! are checked over the typed program.
//!
//! Settings validation is structural only: the block's group shape (children
//! must be groups, a `gamemodes` group must be present), span validity, and
//! non-empty key names. Key *existence* and leaf *kinds* (which keys exist,
//! whether a leaf is a number/bool/string/enum/list-of-heroes, and which
//! mode/team/hero/map/enum spellings are valid) need the canonical Workshop
//! settings schema, which is Workshop-owned catalog content; those checks
//! are `lowering-dependent` (issue #8) and are never approximated with a
//! local allowlist here.

use serde_json::Value;

use super::error::{HirError, invalid};
use super::types::{
    Declaration, Expr, PROTOCOL_MAJOR, PROTOCOL_NAME, Position, Program, Rule, RuleEntry, Settings,
    SettingsNode, Span, Stmt, SwitchArm, default_var_index,
};

/// Declaration `kind` values understood by this consumer.
const DECLARATION_KINDS: &[&str] = &[
    "globalVariable",
    "playerVariable",
    "subroutine",
    "constant",
    "macro",
];
/// Statement `kind` values understood by this consumer.
const STMT_KINDS: &[&str] = &[
    "expr",
    "assign",
    "if",
    "for",
    "while",
    "doWhile",
    "switch",
    "break",
    "callSubroutine",
    "pass",
];
/// Expression `kind` values understood by this consumer.
const EXPR_KINDS: &[&str] = &[
    "number",
    "string",
    "bool",
    "null",
    "array",
    "dict",
    "comprehension",
    "lambda",
    "stringModifier",
    "local",
    "vector",
    "enum",
    "globalVar",
    "playerVar",
    "member",
    "eventPlayer",
    "constant",
    "call",
    "receiverCall",
    "macroCall",
    "macroParam",
    "binary",
    "conditional",
    "unary",
    "index",
    "format",
];
/// Settings node `kind` values understood by this consumer.
const SETTINGS_NODE_KINDS: &[&str] = &["group", "number", "bool", "string", "list"];

/// Declared names, collected before reference validation.
struct NameTables<'a> {
    globals: Vec<&'a str>,
    players: Vec<&'a str>,
    subroutines: Vec<&'a str>,
    constants: Vec<&'a str>,
}

/// Reject any node whose `kind` this consumer does not understand, walking
/// the raw JSON so the error can name the kind and its span. This runs before
/// typed deserialization so an unknown kind cannot be mistaken for malformed
/// JSON.
pub(crate) fn check_unknown_kinds(value: &Value) -> Result<(), HirError> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    if let Some(declarations) = object.get("declarations").and_then(Value::as_array) {
        for declaration in declarations {
            check_declaration(declaration)?;
        }
    }
    if let Some(rules) = object.get("rules").and_then(Value::as_array) {
        for rule in rules {
            check_rule_entry(rule)?;
        }
    }
    if let Some(settings) = object.get("settings") {
        check_settings_value(settings)?;
    }
    Ok(())
}

/// Validate the protocol envelope against the supported identity and major
/// version, before any program-body inspection.
pub(crate) fn check_envelope(value: &Value) -> Result<(), HirError> {
    let name = value
        .get("protocol")
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = value
        .get("protocol")
        .and_then(|p| p.get("version"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let received = if name.is_empty() && version.is_empty() {
        "<missing protocol>".to_string()
    } else {
        format!("{name}@{version}")
    };
    let expected = format!("{PROTOCOL_NAME}@v{PROTOCOL_MAJOR}");
    if name != PROTOCOL_NAME {
        return Err(HirError::IncompatibleProtocol { expected, received });
    }
    let major = version
        .split('.')
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .ok_or_else(|| HirError::IncompatibleProtocol {
            expected: expected.clone(),
            received: received.clone(),
        })?;
    if major != PROTOCOL_MAJOR {
        return Err(HirError::IncompatibleProtocol { expected, received });
    }
    Ok(())
}

/// Validate structural invariants over a typed program: provenance, spans,
/// identifier uniqueness, and reference resolution.
pub(crate) fn validate_program(program: &Program) -> Result<(), HirError> {
    let mut seen_files = std::collections::HashSet::new();
    for file in &program.files {
        if !seen_files.insert(file.id) {
            return Err(invalid(
                "invalid-identifier",
                format!("duplicate file id {}", file.id),
                None,
            ));
        }
    }

    let mut tables = NameTables {
        globals: Vec::new(),
        players: Vec::new(),
        subroutines: Vec::new(),
        constants: Vec::new(),
    };

    let mut macro_names: Vec<&str> = Vec::new();
    let mut define_names: Vec<&str> = Vec::new();

    for declaration in &program.declarations {
        match declaration {
            Declaration::GlobalVariable {
                name,
                span,
                initializer,
                ..
            } => {
                check_name(name, "global variable", *span)?;
                check_unique(name, "global variable", *span, &mut tables.globals)?;
                tables.globals.push(name);
                if let Some(initializer) = initializer {
                    validate_expr(initializer, program, &tables)?;
                }
            }
            Declaration::PlayerVariable {
                name,
                span,
                initializer,
                ..
            } => {
                check_name(name, "player variable", *span)?;
                check_unique(name, "player variable", *span, &mut tables.players)?;
                tables.players.push(name);
                if let Some(initializer) = initializer {
                    validate_expr(initializer, program, &tables)?;
                }
            }
            Declaration::Subroutine { name, span, .. } => {
                check_name(name, "subroutine", *span)?;
                check_unique(name, "subroutine", *span, &mut tables.subroutines)?;
                tables.subroutines.push(name);
            }
            Declaration::Constant { name, span, value } => {
                check_name(name, "constant", *span)?;
                check_unique(name, "constant", *span, &mut tables.constants)?;
                tables.constants.push(name);
                validate_expr(value, program, &tables)?;
            }
            Declaration::Macro {
                name,
                args,
                span,
                body,
            } => {
                check_name(name, "macro", *span)?;
                check_unique(name, "macro", *span, &mut macro_names)?;
                for arg in args {
                    if arg.is_empty() {
                        return Err(invalid(
                            "invalid-identifier",
                            "macro argument name must be non-empty",
                            *span,
                        ));
                    }
                }
                validate_stmts(body, program, &tables, |statement| {
                    statement.span().copied()
                })?;
            }
        }
    }

    for define in &program.defines {
        if define.name.is_empty() {
            return Err(invalid(
                "invalid-identifier",
                "define name must be non-empty",
                define.span,
            ));
        }
        check_span(define.span, program.files.len())?;
        if define_names.contains(&define.name.as_str()) {
            return Err(invalid(
                "invalid-identifier",
                format!("duplicate define name '{}'", define.name),
                define.span,
            ));
        }
        define_names.push(&define.name);
    }

    for entry in &program.rules {
        match entry {
            RuleEntry::Rule(rule) => validate_rule(rule, program, &tables)?,
            RuleEntry::SubroutineDef {
                name,
                source_name,
                span,
                body,
                ..
            } => {
                check_name(name, "subroutine definition", *span)?;
                if !source_name.is_empty() {
                    check_name(source_name, "subroutine source", *span)?;
                }
                validate_stmts(body, program, &tables, |statement| {
                    statement.span().copied()
                })?;
            }
        }
    }

    if let Some(settings) = &program.settings {
        validate_settings(program, settings)?;
    }

    Ok(())
}

/// Validate the settings carrier structurally: the block's group shape,
/// span validity, and non-empty key names. Key existence and leaf kinds are
/// NOT validated here — the canonical Workshop settings schema (key paths,
/// value kinds, mode/team/hero/map/enum spellings) is Workshop-owned catalog
/// content, so that validation is `lowering-dependent` (issue #8) and the
/// tree is carried structurally.
fn validate_settings(program: &Program, settings: &Settings) -> Result<(), HirError> {
    check_span(settings.span, program.files.len())?;
    let mut has_gamemodes = false;
    for child in &settings.children {
        let SettingsNode::Group { name, .. } = child else {
            return Err(invalid(
                "settings-invalid",
                "settings block children must be groups",
                child.span().copied(),
            ));
        };
        if name == "gamemodes" {
            has_gamemodes = true;
        }
        validate_settings_node(program, child)?;
    }
    if !has_gamemodes {
        return Err(invalid(
            "settings-invalid",
            "settings block must contain a gamemodes group",
            settings.span,
        ));
    }
    Ok(())
}

/// Validate one settings node: span validity, non-empty key, and (for
/// groups) recursive child validation. Values are carried structurally —
/// the JSONC parse is value-driven and key kinds are lowering-dependent.
fn validate_settings_node(program: &Program, node: &SettingsNode) -> Result<(), HirError> {
    check_span(node.span().copied(), program.files.len())?;
    check_key(node_name(node), node.span().copied())?;
    if let SettingsNode::Group { children, .. } = node {
        for child in children {
            validate_settings_node(program, child)?;
        }
    }
    Ok(())
}

fn node_name(node: &SettingsNode) -> &str {
    match node {
        SettingsNode::Group { name, .. }
        | SettingsNode::Number { name, .. }
        | SettingsNode::Bool { name, .. }
        | SettingsNode::String { name, .. }
        | SettingsNode::List { name, .. } => name,
    }
}

fn check_key(name: &str, span: Option<Span>) -> Result<(), HirError> {
    if name.is_empty() {
        return Err(invalid(
            "invalid-identifier",
            "settings key must be non-empty",
            span,
        ));
    }
    Ok(())
}

fn validate_rule(rule: &Rule, program: &Program, tables: &NameTables<'_>) -> Result<(), HirError> {
    check_span(rule.span, program.files.len())?;
    if rule.event.name.is_empty() {
        return Err(invalid(
            "invalid-identifier",
            "event name must be non-empty",
            rule.event.span,
        ));
    }
    validate_expr_vec(&rule.event.args, program, tables)?;
    check_span(rule.event.span, program.files.len())?;
    validate_expr_vec(&rule.conditions, program, tables)?;
    validate_stmts(&rule.actions, program, tables, |statement| {
        statement.span().copied()
    })
}

/// Validate a list of owned expressions (used for event args and rule
/// conditions).
fn validate_expr_vec(
    exprs: &[Expr],
    program: &Program,
    tables: &NameTables<'_>,
) -> Result<(), HirError> {
    let refs: Vec<&Expr> = exprs.iter().collect();
    validate_exprs(&refs, program, tables)
}

/// Validate a statement list: every span, every nested expression and its
/// references, subroutine-call targets, and for-loop variables.
fn validate_stmts(
    statements: &[Stmt],
    program: &Program,
    tables: &NameTables<'_>,
    span_of: impl Fn(&Stmt) -> Option<Span>,
) -> Result<(), HirError> {
    let mut errors = Vec::new();
    for_each_stmt(statements, &mut |statement| {
        if let Err(error) = check_span(span_of(statement), program.files.len()) {
            errors.push(error);
        }
        match statement {
            Stmt::CallSubroutine { name, span } => {
                let known = tables.subroutines.contains(&name.as_str())
                    || program.rules.iter().any(|entry| {
                        matches!(
                            entry,
                            RuleEntry::SubroutineDef {
                                name: presentation,
                                source_name,
                                ..
                            } if presentation == name || source_name == name
                        )
                    });
                if !known {
                    errors.push(invalid(
                        "unresolved-reference",
                        format!("call to unknown subroutine '{name}'"),
                        *span,
                    ));
                }
            }
            Stmt::For { variable, span, .. } => match variable.as_ref() {
                Expr::GlobalVar { name, .. }
                    if tables.globals.contains(&name.as_str())
                        || default_var_index(name).is_some() => {}
                Expr::GlobalVar { name, .. } => errors.push(invalid(
                    "unresolved-reference",
                    format!("for-loop variable '{name}' is not a declared global variable"),
                    *span,
                )),
                other => errors.push(invalid(
                    "invalid-structure",
                    format!(
                        "for-loop variable must be a global variable reference, got '{}'",
                        other.kind_name()
                    ),
                    *span,
                )),
            },
            Stmt::Switch { arms, .. } => {
                if arms.is_empty() {
                    errors.push(invalid(
                        "invalid-structure",
                        "a switch must contain at least one arm",
                        None,
                    ));
                }
                let mut defaults = 0;
                for arm in arms {
                    let arm_span = match arm {
                        SwitchArm::Case { span, body, .. } | SwitchArm::Default { span, body } => {
                            let _ = body;
                            span
                        }
                    };
                    if let Err(error) = check_span(*arm_span, program.files.len()) {
                        errors.push(error);
                    }
                    if matches!(arm, SwitchArm::Default { .. }) {
                        defaults += 1;
                        if defaults > 1 {
                            errors.push(invalid(
                                "invalid-structure",
                                "a switch may contain at most one default arm",
                                *arm_span,
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    });
    validate_exprs(&statement_exprs(statements), program, tables)?;
    errors.into_iter().next().map_or(Ok(()), Err)
}

/// Validate a single expression, including its children.
fn validate_expr(expr: &Expr, program: &Program, tables: &NameTables<'_>) -> Result<(), HirError> {
    validate_exprs(&[expr], program, tables)
}

/// Validate every expression in a list, including spans and references.
fn validate_exprs(
    exprs: &[&Expr],
    program: &Program,
    tables: &NameTables<'_>,
) -> Result<(), HirError> {
    let mut errors = Vec::new();
    for expr in exprs {
        for_each_expr(expr, &mut |node| {
            if let Err(error) = check_span(node.span().copied(), program.files.len()) {
                errors.push(error);
            }
            match node {
                Expr::GlobalVar { name, span }
                    if !tables.globals.contains(&name.as_str())
                        && default_var_index(name).is_none() =>
                {
                    errors.push(invalid(
                        "unresolved-reference",
                        format!("reference to unknown global variable '{name}'"),
                        *span,
                    ));
                }
                Expr::PlayerVar { player, name, span }
                    if !tables.players.contains(&name.as_str())
                        && !is_implicit_player_variable(player, name) =>
                {
                    errors.push(invalid(
                        "unresolved-reference",
                        format!("reference to unknown player variable '{name}'"),
                        *span,
                    ));
                }
                Expr::Constant { name, span } if !tables.constants.contains(&name.as_str()) => {
                    errors.push(invalid(
                        "unresolved-reference",
                        format!("reference to unknown constant '{name}'"),
                        *span,
                    ));
                }
                _ => {}
            }
        });
    }
    errors.into_iter().next().map_or(Ok(()), Err)
}

fn is_implicit_player_variable(player: &Expr, name: &str) -> bool {
    matches!(player, Expr::EventPlayer { .. }) && default_var_index(name).is_some()
}

/// The expressions directly contained in a statement list (used to feed
/// `validate_exprs`; nested bodies are covered by `for_each_stmt`).
fn statement_exprs(statements: &[Stmt]) -> Vec<&Expr> {
    let mut exprs = Vec::new();
    for statement in statements {
        match statement {
            Stmt::Expr { expr, .. } => exprs.push(expr.as_ref()),
            Stmt::Assign { target, value, .. } => {
                exprs.push(target.as_ref());
                exprs.push(value.as_ref());
            }
            Stmt::If {
                branches, r#else, ..
            } => {
                for branch in branches {
                    exprs.push(branch.condition.as_ref());
                    exprs.extend(statement_exprs(&branch.body));
                }
                if let Some(else_body) = r#else {
                    exprs.extend(statement_exprs(else_body));
                }
            }
            Stmt::For {
                variable,
                iterable,
                body,
                ..
            } => {
                exprs.push(variable.as_ref());
                exprs.push(iterable.as_ref());
                exprs.extend(statement_exprs(body));
            }
            Stmt::While {
                condition, body, ..
            } => {
                exprs.push(condition.as_ref());
                exprs.extend(statement_exprs(body));
            }
            Stmt::DoWhile {
                condition, body, ..
            } => {
                exprs.push(condition.as_ref());
                exprs.extend(statement_exprs(body));
            }
            Stmt::Switch { value, arms, .. } => {
                exprs.push(value.as_ref());
                for arm in arms {
                    match arm {
                        SwitchArm::Case { value, body, .. } => {
                            exprs.push(value.as_ref());
                            exprs.extend(statement_exprs(body));
                        }
                        SwitchArm::Default { body, .. } => {
                            exprs.extend(statement_exprs(body));
                        }
                    }
                }
            }
            Stmt::Break { .. } | Stmt::CallSubroutine { .. } | Stmt::Pass { .. } => {}
        }
    }
    exprs
}

fn check_name(name: &str, what: &str, span: Option<Span>) -> Result<(), HirError> {
    if name.is_empty() {
        return Err(invalid(
            "invalid-identifier",
            format!("{what} name must be non-empty"),
            span,
        ));
    }
    Ok(())
}

fn check_unique(
    name: &str,
    what: &str,
    span: Option<Span>,
    seen: &mut Vec<&str>,
) -> Result<(), HirError> {
    if seen.contains(&name) {
        return Err(invalid(
            "invalid-identifier",
            format!("duplicate {what} name '{name}'"),
            span,
        ));
    }
    Ok(())
}

fn check_span(span: Option<Span>, files: usize) -> Result<(), HirError> {
    let Some(span) = span else {
        return Ok(());
    };
    if span.file as usize >= files {
        return Err(invalid(
            "invalid-span",
            format!("span references unknown file {}", span.file),
            Some(span),
        ));
    }
    if !valid_position(&span.start) || !valid_position(&span.end) {
        return Err(invalid(
            "invalid-span",
            "span positions are 1-based; line and column must be >= 1",
            Some(span),
        ));
    }
    if span.end.line < span.start.line
        || (span.end.line == span.start.line && span.end.col < span.start.col)
    {
        return Err(invalid(
            "invalid-span",
            "span end precedes span start",
            Some(span),
        ));
    }
    Ok(())
}

fn valid_position(position: &Position) -> bool {
    position.line >= 1 && position.col >= 1
}

/// Visit every statement in a tree (including nested bodies).
fn for_each_stmt<'a>(statements: &'a [Stmt], f: &mut impl FnMut(&'a Stmt)) {
    for statement in statements {
        f(statement);
        match statement {
            Stmt::If {
                branches, r#else, ..
            } => {
                for branch in branches {
                    for_each_stmt(&branch.body, f);
                }
                if let Some(else_body) = r#else {
                    for_each_stmt(else_body, f);
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                for_each_stmt(body, f)
            }
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    match arm {
                        SwitchArm::Case { body, .. } | SwitchArm::Default { body, .. } => {
                            for_each_stmt(body, f);
                        }
                    }
                }
            }
            Stmt::Expr { .. }
            | Stmt::Assign { .. }
            | Stmt::Break { .. }
            | Stmt::CallSubroutine { .. }
            | Stmt::Pass { .. } => {}
        }
    }
}

/// Visit every expression in a list (including nested children).
fn for_each_expr<'a>(expr: &'a Expr, f: &mut impl FnMut(&'a Expr)) {
    f(expr);
    match expr {
        Expr::Array { elements, .. } => {
            for element in elements {
                for_each_expr(element, f);
            }
        }
        Expr::Dict { entries, .. } => {
            for entry in entries {
                for_each_expr(&entry.key, f);
                for_each_expr(&entry.value, f);
            }
        }
        Expr::Comprehension {
            element,
            iterable,
            condition,
            ..
        } => {
            for_each_expr(iterable, f);
            for_each_expr(element, f);
            if let Some(condition) = condition {
                for_each_expr(condition, f);
            }
        }
        Expr::Lambda { body, .. } => for_each_expr(body, f),
        Expr::Vector { x, y, z, .. } => {
            for_each_expr(x, f);
            for_each_expr(y, f);
            for_each_expr(z, f);
        }
        Expr::PlayerVar { player, .. } => for_each_expr(player, f),
        Expr::Member { receiver, .. } => for_each_expr(receiver, f),
        Expr::Call { args, .. } | Expr::MacroCall { args, .. } | Expr::Format { args, .. } => {
            for arg in args {
                for_each_expr(arg, f);
            }
        }
        Expr::ReceiverCall { receiver, args, .. } => {
            for_each_expr(receiver, f);
            for arg in args {
                for_each_expr(arg, f);
            }
        }
        Expr::Binary { left, right, .. } => {
            for_each_expr(left, f);
            for_each_expr(right, f);
        }
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => {
            for_each_expr(then_value, f);
            for_each_expr(condition, f);
            for_each_expr(else_value, f);
        }
        Expr::Unary { operand, .. } => for_each_expr(operand, f),
        Expr::Index { array, index, .. } => {
            for_each_expr(array, f);
            for_each_expr(index, f);
        }
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Null { .. }
        | Expr::Enum { .. }
        | Expr::GlobalVar { .. }
        | Expr::EventPlayer { .. }
        | Expr::Constant { .. }
        | Expr::MacroParam { .. }
        | Expr::StringModifier { .. }
        | Expr::Local { .. } => {}
    }
}

fn check_declaration(value: &Value) -> Result<(), HirError> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
    if !DECLARATION_KINDS.contains(&kind) {
        return Err(unsupported_node(kind, object));
    }
    if let Some(initializer) = object.get("initializer") {
        check_expr(initializer)?;
    }
    if let Some(value_expr) = object.get("value") {
        check_expr(value_expr)?;
    }
    if let Some(body) = object.get("body").and_then(Value::as_array) {
        for statement in body {
            check_stmt(statement)?;
        }
    }
    Ok(())
}

fn check_rule_entry(value: &Value) -> Result<(), HirError> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    match object.get("kind").and_then(Value::as_str) {
        Some(kind) if kind != "subroutineDef" => Err(unsupported_node(kind, object)),
        Some(_) => {
            if let Some(body) = object.get("body").and_then(Value::as_array) {
                for statement in body {
                    check_stmt(statement)?;
                }
            }
            Ok(())
        }
        None => {
            if let Some(event) = object.get("event").and_then(Value::as_object) {
                if let Some(args) = event.get("args").and_then(Value::as_array) {
                    for arg in args {
                        check_expr(arg)?;
                    }
                }
            }
            if let Some(conditions) = object.get("conditions").and_then(Value::as_array) {
                for condition in conditions {
                    check_expr(condition)?;
                }
            }
            if let Some(actions) = object.get("actions").and_then(Value::as_array) {
                for action in actions {
                    check_stmt(action)?;
                }
            }
            Ok(())
        }
    }
}

fn check_stmt(value: &Value) -> Result<(), HirError> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
    if !STMT_KINDS.contains(&kind) {
        return Err(unsupported_node(kind, object));
    }
    for field in [
        "expr",
        "target",
        "value",
        "condition",
        "variable",
        "iterable",
    ] {
        if let Some(child) = object.get(field) {
            check_expr(child)?;
        }
    }
    if let Some(branches) = object.get("branches").and_then(Value::as_array) {
        for branch in branches {
            if let Some(condition) = branch.get("condition") {
                check_expr(condition)?;
            }
            if let Some(body) = branch.get("body").and_then(Value::as_array) {
                for statement in body {
                    check_stmt(statement)?;
                }
            }
        }
    }
    if let Some(arms) = object.get("arms").and_then(Value::as_array) {
        for arm in arms {
            let Some(arm_object) = arm.as_object() else {
                return Err(unsupported_node("", object));
            };
            let arm_kind = arm_object.get("kind").and_then(Value::as_str).unwrap_or("");
            match arm_kind {
                "case" => {
                    if let Some(value) = arm_object.get("value") {
                        check_expr(value)?;
                    }
                }
                "default" => {}
                _ => return Err(unsupported_node(arm_kind, arm_object)),
            }
            if let Some(body) = arm_object.get("body").and_then(Value::as_array) {
                for statement in body {
                    check_stmt(statement)?;
                }
            }
        }
    }
    for field in ["body", "else"] {
        if let Some(statements) = object.get(field).and_then(Value::as_array) {
            for statement in statements {
                check_stmt(statement)?;
            }
        }
    }
    Ok(())
}

fn check_expr(value: &Value) -> Result<(), HirError> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
    if !EXPR_KINDS.contains(&kind) {
        return Err(unsupported_node(kind, object));
    }
    for field in [
        "x",
        "y",
        "z",
        "player",
        "receiver",
        "left",
        "right",
        "operand",
        "array",
        "index",
        "thenValue",
        "elseValue",
    ] {
        if let Some(child) = object.get(field) {
            check_expr(child)?;
        }
    }
    for field in ["args", "elements"] {
        if let Some(children) = object.get(field).and_then(Value::as_array) {
            for child in children {
                check_expr(child)?;
            }
        }
    }
    if let Some(entries) = object.get("entries").and_then(Value::as_array) {
        for entry in entries {
            if let Some(key) = entry.get("key") {
                check_expr(key)?;
            }
            if let Some(entry_value) = entry.get("value") {
                check_expr(entry_value)?;
            }
        }
    }
    if let Some(condition) = object.get("condition") {
        check_expr(condition)?;
    }
    if let Some(body) = object.get("body") {
        check_expr(body)?;
    }
    Ok(())
}

fn check_settings_value(value: &Value) -> Result<(), HirError> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let Some(children) = object.get("children").and_then(Value::as_array) else {
        return Ok(());
    };
    for child in children {
        check_settings_node(child)?;
    }
    Ok(())
}

fn check_settings_node(value: &Value) -> Result<(), HirError> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
    if !SETTINGS_NODE_KINDS.contains(&kind) {
        return Err(unsupported_node(kind, object));
    }
    if let Some(children) = object.get("children").and_then(Value::as_array) {
        for child in children {
            check_settings_node(child)?;
        }
    }
    Ok(())
}

fn unsupported_node(kind: &str, object: &serde_json::Map<String, Value>) -> HirError {
    let span = object
        .get("span")
        .and_then(|value| serde_json::from_value::<Span>(value.clone()).ok());
    HirError::UnsupportedNode {
        kind: kind.to_string(),
        span,
    }
}
