use super::types::{Declaration, Expr, Program, RuleEntry, Span, Stmt, SwitchArm};

pub(crate) fn contains_random(expression: &Expr) -> bool {
    struct RandomFinder(bool);

    impl Visitor for RandomFinder {
        fn visit_expr(&mut self, expression: &Expr) {
            if self.0 {
                return;
            }
            if matches!(
                expression,
                Expr::Call { name, .. } | Expr::MacroCall { name, .. }
                    if name.starts_with("random.")
            ) {
                self.0 = true;
            } else {
                walk_expr(self, expression);
            }
        }
    }

    let mut finder = RandomFinder(false);
    Visitor::visit_expr(&mut finder, expression);
    finder.0
}

pub(crate) fn indexed_expr_parts(expression: &Expr) -> (&Expr, Vec<&Expr>) {
    let mut indices = Vec::new();
    let mut root = expression;
    while let Expr::Index { array, index, .. } = root {
        indices.push(index.as_ref());
        root = array;
    }
    indices.reverse();
    (root, indices)
}

pub(crate) fn literal_number(expression: &Expr) -> Option<f64> {
    match expression {
        Expr::Null { .. } => Some(0.0),
        Expr::Number { value, .. } => Some(*value),
        Expr::Unary { op, operand, .. } if op == "+" => literal_number(operand),
        Expr::Unary { op, operand, .. } if op == "-" => literal_number(operand).map(|value| -value),
        _ => None,
    }
}

pub(crate) fn has_random_nested_delete(root: &Expr, indices: &[&Expr]) -> bool {
    indices.len() >= 3
        && (contains_random(root)
            || indices[..indices.len() - 1]
                .iter()
                .any(|index| contains_random(index)))
}

pub(crate) trait Visitor {
    fn visit_expr(&mut self, expression: &Expr) {
        walk_expr(self, expression);
    }

    fn visit_comprehension(&mut self, element: &Expr, iterable: &Expr, condition: Option<&Expr>) {
        walk_comprehension(self, element, iterable, condition);
    }

    fn visit_stmt(&mut self, statement: &Stmt) {
        walk_stmt(self, statement);
    }
}

pub(crate) fn walk_program<V: Visitor + ?Sized>(visitor: &mut V, program: &Program) {
    for declaration in &program.declarations {
        match declaration {
            Declaration::GlobalVariable {
                initializer: Some(initializer),
                ..
            }
            | Declaration::PlayerVariable {
                initializer: Some(initializer),
                ..
            } => visitor.visit_expr(initializer),
            Declaration::Constant { value, .. } => visitor.visit_expr(value),
            Declaration::Macro { body, .. } => walk_stmts(visitor, body),
            _ => {}
        }
    }
    for entry in &program.rules {
        match entry {
            RuleEntry::Rule(rule) => {
                for argument in &rule.event.args {
                    visitor.visit_expr(argument);
                }
                for condition in &rule.conditions {
                    visitor.visit_expr(condition);
                }
                walk_stmts(visitor, &rule.actions);
            }
            RuleEntry::SubroutineDef { body, .. } => walk_stmts(visitor, body),
        }
    }
}

pub(crate) fn walk_expr<V: Visitor + ?Sized>(visitor: &mut V, expression: &Expr) {
    match expression {
        Expr::Array { elements, .. } => {
            for element in elements {
                visitor.visit_expr(element);
            }
        }
        Expr::Dict { entries, .. } => {
            for entry in entries {
                visitor.visit_expr(&entry.key);
                visitor.visit_expr(&entry.value);
            }
        }
        Expr::Comprehension {
            element,
            iterable,
            condition,
            ..
        } => visitor.visit_comprehension(element, iterable, condition.as_deref()),
        Expr::Lambda { body, .. } => visitor.visit_expr(body),
        Expr::Type { args, .. }
        | Expr::Call { args, .. }
        | Expr::MacroCall { args, .. }
        | Expr::Format { args, .. } => {
            for argument in args {
                visitor.visit_expr(argument);
            }
        }
        Expr::Vector { x, y, z, .. } => {
            visitor.visit_expr(x);
            visitor.visit_expr(y);
            visitor.visit_expr(z);
        }
        Expr::PlayerVar { player, .. }
        | Expr::Member {
            receiver: player, ..
        } => {
            visitor.visit_expr(player);
        }
        Expr::ReceiverCall { receiver, args, .. } => {
            visitor.visit_expr(receiver);
            for argument in args {
                visitor.visit_expr(argument);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Index {
            array: left,
            index: right,
            ..
        } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        }
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => {
            visitor.visit_expr(then_value);
            visitor.visit_expr(condition);
            visitor.visit_expr(else_value);
        }
        Expr::Unary { operand, .. } => visitor.visit_expr(operand),
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Null { .. }
        | Expr::StringModifier { .. }
        | Expr::Local { .. }
        | Expr::Enum { .. }
        | Expr::GlobalVar { .. }
        | Expr::HostPlayer { .. }
        | Expr::EventPlayer { .. }
        | Expr::Constant { .. }
        | Expr::MacroParam { .. } => {}
    }
}

pub(crate) fn walk_comprehension<V: Visitor + ?Sized>(
    visitor: &mut V,
    element: &Expr,
    iterable: &Expr,
    condition: Option<&Expr>,
) {
    visitor.visit_expr(element);
    visitor.visit_expr(iterable);
    if let Some(condition) = condition {
        visitor.visit_expr(condition);
    }
}

pub(crate) fn walk_stmts<V: Visitor + ?Sized>(visitor: &mut V, statements: &[Stmt]) {
    for statement in statements {
        visitor.visit_stmt(statement);
    }
}

pub(crate) fn walk_stmt<V: Visitor + ?Sized>(visitor: &mut V, statement: &Stmt) {
    match statement {
        Stmt::Expr { expr, .. } => visitor.visit_expr(expr),
        Stmt::Assign { target, value, .. } => {
            visitor.visit_expr(target);
            visitor.visit_expr(value);
        }
        Stmt::If {
            branches, r#else, ..
        } => {
            for branch in branches {
                visitor.visit_expr(&branch.condition);
                walk_stmts(visitor, &branch.body);
            }
            if let Some(default_body) = r#else {
                walk_stmts(visitor, default_body);
            }
        }
        Stmt::For {
            variable,
            iterable,
            body,
            ..
        } => {
            visitor.visit_expr(variable);
            visitor.visit_expr(iterable);
            walk_stmts(visitor, body);
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            condition, body, ..
        } => {
            visitor.visit_expr(condition);
            walk_stmts(visitor, body);
        }
        Stmt::Switch { value, arms, .. } => {
            visitor.visit_expr(value);
            for arm in arms {
                match arm {
                    SwitchArm::Case { value, body, .. } => {
                        visitor.visit_expr(value);
                        walk_stmts(visitor, body);
                    }
                    SwitchArm::Default { body, .. } => walk_stmts(visitor, body),
                }
            }
        }
        Stmt::Delete { target, .. } => visitor.visit_expr(target),
        Stmt::Goto { offset, .. } => {
            if let Some(offset) = offset {
                visitor.visit_expr(offset);
            }
        }
        Stmt::Break { .. }
        | Stmt::Return { .. }
        | Stmt::Continue { .. }
        | Stmt::Label { .. }
        | Stmt::CallSubroutine { .. }
        | Stmt::Pass { .. } => {}
    }
}

/// Equality on definition-site spans: macro attribution (see
/// [`Span::site`]) never changes the result, so a lowering decision is the
/// same with and without attribution.
pub(crate) fn same_definition(left: &Expr, right: &Expr) -> bool {
    let all = |left: &[Expr], right: &[Expr]| {
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(left, right)| same_definition(left, right))
    };
    if !same_span(left.span(), right.span()) {
        return false;
    }
    match (left, right) {
        (
            Expr::Number { value, text, .. },
            Expr::Number {
                value: other_value,
                text: other_text,
                ..
            },
        ) => value == other_value && text == other_text,
        (Expr::String { value, .. }, Expr::String { value: other, .. }) => value == other,
        (Expr::Bool { value, .. }, Expr::Bool { value: other, .. }) => value == other,
        (Expr::Null { .. }, Expr::Null { .. })
        | (Expr::HostPlayer { .. }, Expr::HostPlayer { .. })
        | (Expr::EventPlayer { .. }, Expr::EventPlayer { .. }) => true,
        (
            Expr::Array { elements, .. },
            Expr::Array {
                elements: other, ..
            },
        ) => all(elements, other),
        (Expr::Dict { entries, .. }, Expr::Dict { entries: other, .. }) => {
            entries.len() == other.len()
                && entries.iter().zip(other).all(|(entry, other)| {
                    same_span(entry.span.as_ref(), other.span.as_ref())
                        && same_definition(&entry.key, &other.key)
                        && same_definition(&entry.value, &other.value)
                })
        }
        (
            Expr::Comprehension {
                element,
                variable,
                variable_span,
                index,
                index_span,
                iterable,
                condition,
                ..
            },
            Expr::Comprehension {
                element: other_element,
                variable: other_variable,
                variable_span: other_variable_span,
                index: other_index,
                index_span: other_index_span,
                iterable: other_iterable,
                condition: other_condition,
                ..
            },
        ) => {
            variable == other_variable
                && same_span(variable_span.as_ref(), other_variable_span.as_ref())
                && same_span(index_span.as_ref(), other_index_span.as_ref())
                && index == other_index
                && same_definition(element, other_element)
                && same_definition(iterable, other_iterable)
                && match (condition, other_condition) {
                    (Some(condition), Some(other)) => same_definition(condition, other),
                    (condition, other) => condition.is_none() && other.is_none(),
                }
        }
        (
            Expr::Lambda {
                params,
                param_spans,
                body,
                ..
            },
            Expr::Lambda {
                params: other_params,
                param_spans: other_spans,
                body: other_body,
                ..
            },
        ) => {
            params == other_params
                && param_spans.len() == other_spans.len()
                && param_spans
                    .iter()
                    .zip(other_spans)
                    .all(|(span, other)| same_span(span.as_ref(), other.as_ref()))
                && same_definition(body, other_body)
        }
        (
            Expr::StringModifier {
                modifier, value, ..
            },
            Expr::StringModifier {
                modifier: other_modifier,
                value: other_value,
                ..
            },
        ) => modifier == other_modifier && value == other_value,
        (Expr::Local { name, .. }, Expr::Local { name: other, .. })
        | (Expr::GlobalVar { name, .. }, Expr::GlobalVar { name: other, .. })
        | (Expr::Constant { name, .. }, Expr::Constant { name: other, .. })
        | (Expr::MacroParam { name, .. }, Expr::MacroParam { name: other, .. }) => name == other,
        (
            Expr::Vector { x, y, z, .. },
            Expr::Vector {
                x: other_x,
                y: other_y,
                z: other_z,
                ..
            },
        ) => {
            same_definition(x, other_x)
                && same_definition(y, other_y)
                && same_definition(z, other_z)
        }
        (
            Expr::Enum {
                value_type,
                value,
                literal,
                ..
            },
            Expr::Enum {
                value_type: other_type,
                value: other_value,
                literal: other_literal,
                ..
            },
        ) => value_type == other_type && value == other_value && literal == other_literal,
        (
            Expr::PlayerVar {
                player,
                name,
                member_span,
                ..
            },
            Expr::PlayerVar {
                player: other_player,
                name: other_name,
                member_span: other_member_span,
                ..
            },
        ) => {
            name == other_name
                && same_span(member_span.as_ref(), other_member_span.as_ref())
                && same_definition(player, other_player)
        }
        (
            Expr::Member {
                receiver,
                member,
                member_span,
                ..
            },
            Expr::Member {
                receiver: other_receiver,
                member: other_member,
                member_span: other_member_span,
                ..
            },
        ) => {
            member == other_member
                && same_span(member_span.as_ref(), other_member_span.as_ref())
                && same_definition(receiver, other_receiver)
        }
        (
            Expr::Call {
                name,
                args,
                debug_source,
                ..
            },
            Expr::Call {
                name: other_name,
                args: other_args,
                debug_source: other_source,
                ..
            },
        ) => name == other_name && debug_source == other_source && all(args, other_args),
        (
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            },
            Expr::ReceiverCall {
                receiver: other_receiver,
                name: other_name,
                args: other_args,
                ..
            },
        ) => {
            name == other_name && same_definition(receiver, other_receiver) && all(args, other_args)
        }
        (
            Expr::MacroCall { name, args, .. },
            Expr::MacroCall {
                name: other,
                args: other_args,
                ..
            },
        )
        | (
            Expr::Type { name, args, .. },
            Expr::Type {
                name: other,
                args: other_args,
                ..
            },
        ) => name == other && all(args, other_args),
        (
            Expr::Binary {
                op, left, right, ..
            },
            Expr::Binary {
                op: other_op,
                left: other_left,
                right: other_right,
                ..
            },
        ) => {
            op == other_op
                && same_definition(left, other_left)
                && same_definition(right, other_right)
        }
        (
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                ..
            },
            Expr::Conditional {
                then_value: other_then,
                condition: other_condition,
                else_value: other_else,
                ..
            },
        ) => {
            same_definition(then_value, other_then)
                && same_definition(condition, other_condition)
                && same_definition(else_value, other_else)
        }
        (
            Expr::Unary { op, operand, .. },
            Expr::Unary {
                op: other_op,
                operand: other_operand,
                ..
            },
        ) => op == other_op && same_definition(operand, other_operand),
        (
            Expr::Index { array, index, .. },
            Expr::Index {
                array: other_array,
                index: other_index,
                ..
            },
        ) => same_definition(array, other_array) && same_definition(index, other_index),
        (
            Expr::Format { text, args, .. },
            Expr::Format {
                text: other,
                args: other_args,
                ..
            },
        ) => text == other && all(args, other_args),
        _ => false,
    }
}

fn same_span(left: Option<&Span>, right: Option<&Span>) -> bool {
    Span::definition(left.copied()) == Span::definition(right.copied())
}
