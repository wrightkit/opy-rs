//! Deterministic debug/pretty dump for Opy HIR v1 programs.
//!
//! The dump is a stable, human-readable rendering intended for tests and
//! issue reports. It is not part of the wire contract: the same validated
//! payload always produces the same dump, in payload order.

use super::types::{Declaration, Event, Expr, Program, Rule, RuleEntry, SettingsNode, Span, Stmt};

/// Render a validated program as a deterministic text dump.
pub fn dump(program: &Program) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "program {} {}\n",
        program.protocol.name, program.protocol.version
    ));
    out.push_str(&format!(
        "generator {} {} (frontend {})\n",
        program.generator.name, program.generator.version, program.generator.frontend
    ));

    out.push_str("files:\n");
    for file in &program.files {
        out.push_str(&format!("  {} {}\n", file.id, file.path));
    }

    if !program.defines.is_empty() {
        out.push_str("defines:\n");
        for define in &program.defines {
            out.push_str(&format!(
                "  {} ({}){}\n",
                define.name,
                if define.is_function {
                    "function"
                } else {
                    "constant"
                },
                span_suffix(define.span.as_ref()),
            ));
        }
    }

    out.push_str("declarations:\n");
    for declaration in &program.declarations {
        dump_declaration(declaration, &mut out, 1);
    }

    if let Some(settings) = &program.settings {
        out.push_str("settings:\n");
        for node in &settings.children {
            dump_settings_node(node, &mut out, 1);
        }
    }

    out.push_str("rules:\n");
    for entry in &program.rules {
        match entry {
            RuleEntry::Rule(rule) => dump_rule(rule, &mut out, 1),
            RuleEntry::SubroutineDef {
                name, span, body, ..
            } => {
                out.push_str(&format!(
                    "{}subroutineDef {}{}\n",
                    indent(1),
                    name,
                    span_suffix(span.as_ref())
                ));
                dump_stmts(body, &mut out, 2);
            }
        }
    }
    out
}

fn dump_declaration(declaration: &Declaration, out: &mut String, level: usize) {
    match declaration {
        Declaration::GlobalVariable {
            name,
            index,
            span,
            initializer,
            ..
        } => {
            out.push_str(&format!(
                "{}globalVariable {} (index {}){}",
                indent(level),
                name,
                index.map_or("-".to_string(), |i| i.to_string()),
                span_suffix(span.as_ref()),
            ));
            if let Some(initializer) = initializer {
                out.push_str(" = ");
                render_expr(initializer, out);
            }
            out.push('\n');
        }
        Declaration::PlayerVariable {
            name,
            index,
            span,
            initializer,
            ..
        } => {
            out.push_str(&format!(
                "{}playerVariable {} (index {}){}",
                indent(level),
                name,
                index.map_or("-".to_string(), |i| i.to_string()),
                span_suffix(span.as_ref()),
            ));
            if let Some(initializer) = initializer {
                out.push_str(" = ");
                render_expr(initializer, out);
            }
            out.push('\n');
        }
        Declaration::Subroutine {
            name, index, span, ..
        } => {
            out.push_str(&format!(
                "{}subroutine {} (index {}){}\n",
                indent(level),
                name,
                index.map_or("-".to_string(), |i| i.to_string()),
                span_suffix(span.as_ref()),
            ));
        }
        Declaration::Constant { name, span, value } => {
            out.push_str(&format!(
                "{}constant {}{} = ",
                indent(level),
                name,
                span_suffix(span.as_ref())
            ));
            render_expr(value, out);
            out.push('\n');
        }
        Declaration::Macro {
            name,
            args,
            span,
            body,
        } => {
            let signature = if args.is_empty() {
                name.clone()
            } else {
                format!("{name}({})", args.join(", "))
            };
            out.push_str(&format!(
                "{}macro {}{}\n",
                indent(level),
                signature,
                span_suffix(span.as_ref())
            ));
            dump_stmts(body, out, level + 1);
        }
    }
}

fn dump_rule(rule: &Rule, out: &mut String, level: usize) {
    let state = if rule.disabled { " (disabled)" } else { "" };
    out.push_str(&format!(
        "{}rule \"{}\"{}{}\n",
        indent(level),
        rule.name,
        state,
        span_suffix(rule.span.as_ref()),
    ));
    dump_event(&rule.event, out, level + 1);
    for condition in &rule.conditions {
        out.push_str(&format!("{}condition ", indent(level + 1)));
        render_expr(condition, out);
        out.push_str(&format!("{}\n", span_suffix(condition.span())));
    }
    dump_stmts(&rule.actions, out, level + 1);
}

fn dump_event(event: &Event, out: &mut String, level: usize) {
    out.push_str(&format!(
        "{}event {}{}\n",
        indent(level),
        event.name,
        span_suffix(event.span.as_ref()),
    ));
    if !event.args.is_empty() {
        for arg in &event.args {
            out.push_str(&format!("{}arg ", indent(level + 1)));
            render_expr(arg, out);
            out.push('\n');
        }
    }
}

fn dump_stmts(statements: &[Stmt], out: &mut String, level: usize) {
    for statement in statements {
        dump_stmt(statement, out, level);
    }
}

fn dump_stmt(statement: &Stmt, out: &mut String, level: usize) {
    match statement {
        Stmt::Expr { expr, span } => {
            out.push_str(&format!("{}expr ", indent(level)));
            render_expr(expr, out);
            out.push_str(&format!("{}\n", span_suffix(span.as_ref())));
        }
        Stmt::Assign {
            target,
            value,
            span,
        } => {
            out.push_str(&format!("{}assign ", indent(level)));
            render_expr(target, out);
            out.push_str(" = ");
            render_expr(value, out);
            out.push_str(&format!("{}\n", span_suffix(span.as_ref())));
        }
        Stmt::If {
            branches,
            r#else,
            span,
        } => {
            out.push_str(&format!(
                "{}if{}\n",
                indent(level),
                span_suffix(span.as_ref())
            ));
            for (index, branch) in branches.iter().enumerate() {
                out.push_str(&format!(
                    "{}  {}condition ",
                    indent(level),
                    if index == 0 { "" } else { "elif " }
                ));
                render_expr(&branch.condition, out);
                out.push('\n');
                dump_stmts(&branch.body, out, level + 2);
            }
            if let Some(else_body) = r#else {
                out.push_str(&format!("{}  else\n", indent(level)));
                dump_stmts(else_body, out, level + 2);
            }
        }
        Stmt::For {
            variable,
            iterable,
            body,
            span,
        } => {
            out.push_str(&format!("{}for ", indent(level)));
            render_expr(variable, out);
            out.push_str(" in ");
            render_expr(iterable, out);
            out.push_str(&format!("{}\n", span_suffix(span.as_ref())));
            dump_stmts(body, out, level + 1);
        }
        Stmt::While {
            condition,
            body,
            span,
        } => {
            out.push_str(&format!("{}while ", indent(level)));
            render_expr(condition, out);
            out.push_str(&format!("{}\n", span_suffix(span.as_ref())));
            dump_stmts(body, out, level + 1);
        }
        Stmt::CallSubroutine { name, span } => {
            out.push_str(&format!(
                "{}callSubroutine {}{}\n",
                indent(level),
                name,
                span_suffix(span.as_ref())
            ));
        }
        Stmt::Pass { span } => {
            out.push_str(&format!(
                "{}pass{}\n",
                indent(level),
                span_suffix(span.as_ref())
            ));
        }
    }
}

fn dump_settings_node(node: &SettingsNode, out: &mut String, level: usize) {
    match node {
        SettingsNode::Group {
            name,
            children,
            span,
        } => {
            out.push_str(&format!(
                "{}group {}{}\n",
                indent(level),
                name,
                span_suffix(span.as_ref())
            ));
            for child in children {
                dump_settings_node(child, out, level + 1);
            }
        }
        SettingsNode::Number { name, value, span } => {
            out.push_str(&format!(
                "{}number {} {}{}\n",
                indent(level),
                name,
                format_number(*value),
                span_suffix(span.as_ref())
            ));
        }
        SettingsNode::Bool { name, value, span } => {
            out.push_str(&format!(
                "{}bool {} {}{}\n",
                indent(level),
                name,
                value,
                span_suffix(span.as_ref())
            ));
        }
        SettingsNode::String { name, value, span } => {
            out.push_str(&format!(
                "{}string {} {:?}{}\n",
                indent(level),
                name,
                value,
                span_suffix(span.as_ref())
            ));
        }
        SettingsNode::List {
            name,
            elements,
            span,
        } => {
            out.push_str(&format!(
                "{}list {}{}\n",
                indent(level),
                name,
                span_suffix(span.as_ref())
            ));
            for element in elements {
                out.push_str(&format!(
                    "{}  element {}{}\n",
                    indent(level),
                    element.value,
                    span_suffix(element.span.as_ref())
                ));
            }
        }
    }
}

/// Render an expression in a compact, deterministic form.
fn render_expr(expr: &Expr, out: &mut String) {
    match expr {
        Expr::Number { value, .. } => out.push_str(&format_number(*value)),
        Expr::String { value, .. } => out.push_str(&format!("{:?}", value)),
        Expr::Bool { value, .. } => out.push_str(if *value { "true" } else { "false" }),
        Expr::Null { .. } => out.push_str("null"),
        Expr::Array { elements, .. } => {
            out.push('[');
            for (index, element) in elements.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_expr(element, out);
            }
            out.push(']');
        }
        Expr::Vector { x, y, z, .. } => {
            out.push_str("vect(");
            render_expr(x, out);
            out.push_str(", ");
            render_expr(y, out);
            out.push_str(", ");
            render_expr(z, out);
            out.push(')');
        }
        Expr::Enum {
            value_type, value, ..
        } => {
            out.push_str(value_type);
            out.push('.');
            out.push_str(value);
        }
        Expr::GlobalVar { name, .. } => out.push_str(name),
        Expr::PlayerVar { player, name, .. } => {
            render_expr(player, out);
            out.push('.');
            out.push_str(name);
        }
        Expr::EventPlayer { .. } => out.push_str("eventPlayer"),
        Expr::Constant { name, .. } => out.push_str(name),
        Expr::Call { name, args, .. } => {
            out.push_str(name);
            out.push('(');
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_expr(arg, out);
            }
            out.push(')');
        }
        Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } => {
            render_expr(receiver, out);
            out.push('.');
            out.push_str(name);
            out.push('(');
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_expr(arg, out);
            }
            out.push(')');
        }
        Expr::MacroCall { name, args, .. } => {
            out.push('$');
            out.push_str(name);
            out.push('(');
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_expr(arg, out);
            }
            out.push(')');
        }
        Expr::MacroParam { name, .. } => {
            out.push('$');
            out.push_str(name);
        }
        Expr::Binary {
            op, left, right, ..
        } => {
            out.push('(');
            render_expr(left, out);
            out.push(' ');
            out.push_str(op);
            out.push(' ');
            render_expr(right, out);
            out.push(')');
        }
        Expr::Unary { op, operand, .. } => {
            if op == "-" {
                out.push('-');
                render_expr(operand, out);
            } else {
                out.push_str(op);
                out.push(' ');
                render_expr(operand, out);
            }
        }
        Expr::Index { array, index, .. } => {
            render_expr(array, out);
            out.push('[');
            render_expr(index, out);
            out.push(']');
        }
        Expr::Format { text, args, .. } => {
            out.push_str("format(");
            out.push_str(&format!("{:?}", text));
            for arg in args {
                out.push_str(", ");
                render_expr(arg, out);
            }
            out.push(')');
        }
    }
}

fn format_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

fn span_suffix(span: Option<&Span>) -> String {
    match span {
        Some(span) => format!(
            " @{}:{}:{}-{}:{}",
            span.file, span.start.line, span.start.col, span.end.line, span.end.col
        ),
        None => String::new(),
    }
}
