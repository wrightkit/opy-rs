use super::types::{Expr, Stmt, SwitchArm};

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
