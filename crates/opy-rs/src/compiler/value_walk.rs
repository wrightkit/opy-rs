use workshop_rs::Value;

pub(super) fn for_each_child(value: &mut Value, mut visit: impl FnMut(&mut Value)) {
    match value {
        Value::Array(children) | Value::Call { args: children, .. } => {
            for child in children {
                visit(child);
            }
        }
        Value::Vector { x, y, z } => {
            visit(x);
            visit(y);
            visit(z);
        }
        Value::PlayerVariable { player, .. } => visit(player),
        _ => {}
    }
}

pub(super) fn uses_call(value: &Value, name: &str) -> bool {
    match value {
        Value::Call { name: id, args } => id == name || args.iter().any(|arg| uses_call(arg, name)),
        Value::Array(elements) => elements.iter().any(|element| uses_call(element, name)),
        Value::Vector { x, y, z } => uses_call(x, name) || uses_call(y, name) || uses_call(z, name),
        Value::PlayerVariable { player, .. } => uses_call(player, name),
        _ => false,
    }
}
