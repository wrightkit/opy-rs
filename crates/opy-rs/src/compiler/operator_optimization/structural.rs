use workshop_rs::Value;

const RANDOM_CALLS: [&str; 4] = [
    "randomInteger",
    "randomReal",
    "randomValueInArray",
    "randomizedArray",
];

pub(in crate::compiler) fn same(left: &Value, right: &Value) -> bool {
    same_with_random(left, right, false)
}

pub(in crate::compiler) fn same_shape(left: &Value, right: &Value) -> bool {
    same_with_random(left, right, true)
}

fn same_with_random(left: &Value, right: &Value, allow_random: bool) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b))
        | (Value::LocalizedString(a), Value::LocalizedString(b))
        | (Value::GlobalVariable(a), Value::GlobalVariable(b))
        | (Value::Subroutine(a), Value::Subroutine(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::EventPlayer, Value::EventPlayer) => true,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| same_with_random(a, b, allow_random))
        }
        (
            Value::Vector {
                x: ax,
                y: ay,
                z: az,
            },
            Value::Vector {
                x: bx,
                y: by,
                z: bz,
            },
        ) => {
            same_with_random(ax, bx, allow_random)
                && same_with_random(ay, by, allow_random)
                && same_with_random(az, bz, allow_random)
        }
        (
            Value::Enum {
                value_type: at,
                value: av,
            },
            Value::Enum {
                value_type: bt,
                value: bv,
            },
        ) => at == bt && av == bv,
        (
            Value::PlayerVariable {
                player: ap,
                variable: av,
            },
            Value::PlayerVariable {
                player: bp,
                variable: bv,
            },
        ) => av == bv && same_with_random(ap, bp, allow_random),
        (Value::Call { name: an, args: aa }, Value::Call { name: bn, args: ba }) => {
            an == bn
                && (allow_random || !RANDOM_CALLS.contains(&an.as_str()))
                && aa.len() == ba.len()
                && aa
                    .iter()
                    .zip(ba)
                    .all(|(a, b)| same_with_random(a, b, allow_random))
        }
        _ => false,
    }
}
