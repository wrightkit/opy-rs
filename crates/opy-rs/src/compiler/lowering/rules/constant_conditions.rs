use super::*;

impl Lowering<'_> {
    pub(super) fn split_rule_condition<'expr>(
        expr: &'expr Expr,
        conditions: &mut Vec<&'expr Expr>,
    ) {
        match expr {
            Expr::Binary {
                op, left, right, ..
            } if op == "and" => {
                Self::split_rule_condition(left, conditions);
                Self::split_rule_condition(right, conditions);
            }
            Expr::Binary {
                op, left, right, ..
            } if op == "=="
                && matches!(right.as_ref(), Expr::Bool { value: true, .. })
                && matches!(left.as_ref(), Expr::Binary { op, .. } if op == "and") =>
            {
                Self::split_rule_condition(left, conditions);
            }
            _ => conditions.push(expr),
        }
    }

    /// Optimizes each rule condition. The pinned reference drops a condition
    /// that folds to true and drops the whole rule when one folds to false, so
    /// `None` means the rule is not emitted. A `@Delimiter` rule survives and
    /// keeps its false condition.
    pub(super) fn optimized_rule_conditions(
        &self,
        conditions: &[ValueId],
        exprs: &[&Expr],
        delimiter: bool,
    ) -> Option<(Vec<workshop_rs::Condition>, Vec<Option<HirSpan>>)> {
        let mut kept = Vec::new();
        let mut spans = Vec::new();
        for (value, expr) in conditions.iter().zip(exprs) {
            let mut condition = self.materialize_value(*value);
            split_all(&mut condition);
            let optimization = self.optimization_state_at(expr.span());
            if optimization.enabled && optimization.for_size {
                SizeOptimizer::new(self).condition(&mut condition);
            }
            let optimizer = OperatorOptimizer::new(self.compiler, optimization.strict);
            if optimization.enabled {
                match optimizer.constant_truth(&condition) {
                    Some(true) => continue,
                    Some(false) if !delimiter => return None,
                    Some(false) | None => {}
                }
            }
            // The pinned reference wraps a bare Boolean literal as `!= False`.
            let mut condition = match condition {
                workshop_rs::Value::Bool(_) => {
                    workshop_rs::Value::call("!=", [condition, workshop_rs::Value::Bool(false)])
                }
                condition => optimizer.wrap_condition(condition),
            };
            trim_numbers(&mut condition);
            kept.push(workshop_rs::Condition::new(condition));
            spans.push(expr.span().copied());
        }
        Some((kept, spans))
    }
}
