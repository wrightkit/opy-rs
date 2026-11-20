use super::*;
use crate::compiler::operator_optimization::{
    OperatorOptimizer, reads_indexed_target, same_shape, self_modification,
};
use crate::compiler::value_walk::for_each_child;

fn optimize_value(value: &mut Value, optimizer: &OperatorOptimizer<'_>) {
    for_each_child(value, |child| optimize_value(child, optimizer));
    *value = optimizer.node(std::mem::replace(value, Value::Null));
}

fn folded_self_read(
    value: &mut Value,
    optimizer: &OperatorOptimizer<'_>,
    variable: &Value,
    index: &Value,
) -> bool {
    let mut found = reads_indexed_target(value, variable, index);
    for_each_child(value, |child| {
        found |= folded_self_read(child, optimizer, variable, index);
    });
    found |= reads_indexed_target(value, variable, index);
    *value = optimizer.node(std::mem::replace(value, Value::Null));
    found || reads_indexed_target(value, variable, index)
}

fn folded_set_read(args: &[Value], catalog: &Catalog) -> bool {
    let [variable, index, value] = args else {
        return false;
    };
    let optimizer = OperatorOptimizer::with_catalog(catalog, true);
    let mut optimized_variable = variable.clone();
    let mut optimized_index = index.clone();
    optimize_value(&mut optimized_variable, &optimizer);
    optimize_value(&mut optimized_index, &optimizer);
    let mut optimized_value = value.clone();
    let self_read = folded_self_read(
        &mut optimized_value,
        &optimizer,
        &optimized_variable,
        &optimized_index,
    );
    self_read
        && (!same_shape(variable, &optimized_variable)
            || !same_shape(index, &optimized_index)
            || !same_shape(value, &optimized_value))
}

impl<'a> Emitter<'a> {
    pub(super) fn refuse_set_rewrite(&mut self, action: &Action, span: Option<Span>) -> bool {
        let modification = self_modification(action);
        let message = match (action, modification) {
            (Action::Call { name, args }, modification)
                if matches!(
                    name.as_str(),
                    "setGlobalVariableAtIndex" | "setPlayerVariableAtIndex"
                ) =>
            {
                let Some(Value::Call {
                    name: operation, ..
                }) = args.get(2)
                else {
                    return false;
                };
                if modification.is_none() && !folded_set_read(args, self.catalog) {
                    return false;
                }
                format!(
                    "'{name}' with '{operation}' on its own indexed target may not recompile equivalently"
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
