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
