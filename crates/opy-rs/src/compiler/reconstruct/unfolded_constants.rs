use super::{Action, Condition, Emitter, Span, Value};
use crate::compiler::operator_optimization::{OperatorOptimizer, same_shape};
use crate::compiler::value_walk::{for_each_child, uses_call};

impl Emitter<'_> {
    pub(super) fn emit_action(
        &mut self,
        rule_index: usize,
        action_index: usize,
        action: &Action,
        level: usize,
    ) {
        let start = self.out.len();
        let issues = self.issues.len();
        self.emit_single_action(rule_index, action_index, action, level);
        if self.issues.len() != issues || !self.checks_optimizations(rule_index) {
            return;
        }

        let (name, receiver, values) = match action {
            Action::SetGlobalVariable { value, .. } => {
                ("setGlobalVariable", None, std::slice::from_ref(value))
            }
            Action::SetPlayerVariable { player, value, .. } => (
                "setPlayerVariable",
                Some(player),
                std::slice::from_ref(value),
            ),
            Action::ModifyGlobalVariable { value, .. } => {
                ("modifyGlobalVariable", None, std::slice::from_ref(value))
            }
            Action::ModifyPlayerVariable { player, value, .. } => (
                "modifyPlayerVariable",
                Some(player),
                std::slice::from_ref(value),
            ),
            Action::Call { name, args } => (name.as_str(), None, args.as_slice()),
            _ => return,
        };
        let mut values = receiver.into_iter().chain(values);
        if !values
            .clone()
            .any(|value| self.rewrites_in_strict_mode(value))
        {
            return;
        }
        let refusal = match name {
            "setGlobalVariable"
            | "setPlayerVariable"
            | "setGlobalVariableAtIndex"
            | "setPlayerVariableAtIndex" => values
                .any(|value| uses_call(value, "firstOf"))
                .then_some("its 'firstOf' needs optimizations enabled"),
            _ => Some("reconstruction has no unoptimized spelling for this action"),
        };
        if let Some(reason) = refusal {
            self.refuse_optimization_change(
                name,
                reason,
                self.program.action_span(rule_index, action_index),
            );
            return;
        }
        let indent = Self::indent(level);
        self.out
            .insert_str(start, &format!("{indent}#!disableOptimizations\n"));
        self.out.push_str(&indent);
        self.out.push_str("#!enableOptimizations\n");
    }

    pub(super) fn emit_condition_line(
        &mut self,
        rule_index: usize,
        condition_index: usize,
        condition: &Condition,
    ) {
        let span = self.program.condition_span(rule_index, condition_index);
        if self.checks_optimizations(rule_index) && !condition.disabled {
            let optimizer = OperatorOptimizer::with_catalog(self.catalog, true);
            let mut optimized = condition.value.clone();
            optimize_value(&mut optimized, &optimizer);
            if !same_shape(&condition.value, &optimized)
                && (optimizer.constant_truth(&optimized).is_some()
                    || !same_shape(&condition.value, &optimizer.wrap_condition(optimized)))
            {
                self.refuse_optimization_change(
                    "rule condition",
                    "reconstruction has no unoptimized spelling for this construct",
                    span,
                );
                return;
            }
        }
        self.out.push_str("    @Condition ");
        self.emit_value(&condition.value, span);
        self.out.push('\n');
    }

    pub(super) fn check_block_header_rewrites(
        &mut self,
        rule_index: usize,
        action: &Action,
        span: Option<Span>,
    ) {
        if !self.checks_optimizations(rule_index) {
            return;
        }
        let rewrites = |values: &[&Value]| {
            values
                .iter()
                .any(|value| self.rewrites_in_strict_mode(value))
        };
        let (name, changed) = match action {
            Action::If { condition } => ("if", rewrites(&[condition])),
            Action::ElseIf { condition } => ("elseIf", rewrites(&[condition])),
            Action::While { condition } => ("while", rewrites(&[condition])),
            Action::ForGlobalVariable {
                start, stop, step, ..
            } => ("forGlobalVariable", rewrites(&[start, stop, step])),
            Action::ForPlayerVariable {
                player,
                start,
                stop,
                step,
                ..
            } => ("forPlayerVariable", rewrites(&[player, start, stop, step])),
            _ => return,
        };
        if changed {
            self.refuse_optimization_change(
                name,
                "reconstruction has no unoptimized spelling for this construct",
                span,
            );
        }
    }

    fn checks_optimizations(&self, rule_index: usize) -> bool {
        !self.commented && !self.program.rules[rule_index].disabled
    }

    fn refuse_optimization_change(&mut self, name: &str, reason: &str, span: Option<Span>) {
        self.issue(
            "unsupported-value-call",
            format!("cannot preserve the expression in '{name}' because {reason}"),
            span,
        );
    }

    pub(super) fn rewrites_in_strict_mode(&self, value: &Value) -> bool {
        let mut optimized = value.clone();
        optimize_value(
            &mut optimized,
            &OperatorOptimizer::with_catalog(self.catalog, true),
        );
        !same_shape(value, &optimized)
    }
}

pub(super) fn optimize_value(value: &mut Value, optimizer: &OperatorOptimizer<'_>) {
    for_each_child(value, |child| optimize_value(child, optimizer));
    *value = optimizer.node(std::mem::replace(value, Value::Null));
}
