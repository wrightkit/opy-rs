use super::{is_number, same};
use workshop_rs::Value;

pub(in crate::compiler) fn reads_indexed_target(
    read: &Value,
    variable: &Value,
    index: &Value,
) -> bool {
    match read {
        Value::Call { name, args } if name == "valueInArray" && args.len() == 2 => {
            same(&args[0], variable) && same(&args[1], index)
        }
        Value::Call { name, args } if name == "firstOf" && args.len() == 1 => {
            same(&args[0], variable) && is_number(index, 0.0)
        }
        _ => false,
    }
}
