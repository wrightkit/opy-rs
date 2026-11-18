use super::*;
use crate::compiler::operator_optimization::self_modification;

impl<'a> Emitter<'a> {
    /// A Set whose `removeFromArray` operand is its own target (`x`, or
    /// `x[i]` with the same index value) recompiles to the Modify form, so
    /// OPY source cannot reproduce it. The match is the optimizer's own
    /// structural predicate, never a comparison of rendered text.
    pub(super) fn refuse_self_removal(&mut self, action: &Action, span: Option<Span>) -> bool {
        let removal = match self_modification(action) {
            Some(Action::ModifyGlobalVariable { op, .. })
            | Some(Action::ModifyPlayerVariable { op, .. }) => {
                op == ModifyOp::RemoveFromArrayByValue
            }
            Some(Action::Call { args, .. }) => {
                matches!(args.get(2), Some(Value::Call { name, .. }) if name == "removeFromArray")
            }
            _ => false,
        };
        if removal {
            self.issue(
                "unsupported-value-call",
                "value call 'removeFromArray' on its own assignment target recompiles to \
                 a modify action on the OPY surface",
                span,
            );
        }
        removal
    }

    pub(super) fn indexed_target(
        &mut self,
        variable: &Value,
        span: Option<Span>,
    ) -> Option<String> {
        match variable {
            Value::GlobalVariable(name) => Some(name.clone()),
            Value::PlayerVariable { player, variable } => {
                let receiver = self.render_value_as(player, span, |this, player, span| {
                    this.emit_player_variable_receiver(player, span);
                });
                Some(format!("{receiver}.{variable}"))
            }
            _ => None,
        }
    }
}
