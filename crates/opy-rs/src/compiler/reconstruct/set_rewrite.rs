use super::*;
use crate::compiler::operator_optimization::self_modification;

impl<'a> Emitter<'a> {
    pub(super) fn refuse_set_rewrite(&mut self, action: &Action, span: Option<Span>) -> bool {
        let message = match (action, self_modification(action)) {
            (Action::Call { name, args }, Some(Action::Call { name: modify, .. })) => {
                let Some(Value::Call {
                    name: operation, ..
                }) = args.get(2)
                else {
                    return false;
                };
                format!(
                    "'{name}' with '{operation}' on its own indexed target recompiles to '{modify}'"
                )
            }
            (
                _,
                Some(
                    Action::ModifyGlobalVariable { op, .. }
                    | Action::ModifyPlayerVariable { op, .. },
                ),
            ) if op == ModifyOp::RemoveFromArrayByValue => {
                "value call 'removeFromArray' on its own assignment target recompiles to \
                 a modify action on the OPY surface"
                    .to_owned()
            }
            _ => return false,
        };
        self.issue("unsupported-value-call", message, span);
        true
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
