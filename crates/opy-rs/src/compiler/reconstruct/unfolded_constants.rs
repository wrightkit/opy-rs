use super::{Action, Emitter, Value};
use crate::compiler::operator_optimization::OperatorOptimizer;
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
        if self.issues.len() != issues || self.program.rules[rule_index].disabled {
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
        if !values.clone().any(|value| self.folds_in_strict_mode(value)) {
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
            self.issue(
                "unsupported-value-call",
                format!("cannot preserve the unfolded expression in '{name}' because {reason}"),
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

    pub(super) fn folds_in_strict_mode(&self, value: &Value) -> bool {
        optimize_value(
            &mut value.clone(),
            &OperatorOptimizer::with_catalog(self.catalog, true),
        )
    }
}

pub(super) fn optimize_value(value: &mut Value, optimizer: &OperatorOptimizer<'_>) -> bool {
    let mut folded_child = false;
    for_each_child(value, |child| {
        folded_child |= optimize_value(child, optimizer)
    });
    let was_call = matches!(value, Value::Call { .. });
    *value = optimizer.node(std::mem::replace(value, Value::Null));
    folded_child
        || was_call
            && matches!(
                value,
                Value::Number(_)
                    | Value::Bool(_)
                    | Value::Null
                    | Value::String(_)
                    | Value::Enum { .. }
                    | Value::Vector { .. }
            )
}
